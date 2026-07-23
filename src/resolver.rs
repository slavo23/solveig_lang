use std::collections::{HashMap, HashSet};
use std::env;
use std::ops::Range;

use base::{Session, Span as SourceSpan, Symbol};
use core::ast::{
    BinOp, CaseArm, CaseArmNode, ClassDefinition, ClassMethodDef, ConstructorDef, DataDefinition,
    InstanceDefinition, InstanceMethodDef, Kind, Literal, Pattern, PatternNode, Predicate,
    ResolvedModule, Scheme, TermDefinition, TermExpr, TermExprNode, TypeExpr, UnOp,
};
use surface::ast::{
    ModuleName, NameRef, ParsedModule, SpannedWrapper, SurfaceBinOp, SurfaceDefinition,
    SurfaceExpr, SurfaceLiteral, SurfaceMatchArm, SurfacePattern, SurfacePredicate, SurfaceTy,
    SurfaceUnOp,
};
use syntax::data_kind_infer::{infer_all_data_kinds_with_env, RawConstructorDef, RawDataDef};
use syntax::inference::class_kind_infer::ClassKindInferencer;
use syntax::signature_infer::{KindError, SignatureKindInferencer};

use crate::project::ModuleContext;

type Span = Range<usize>;

fn to_core_span(span: SourceSpan) -> Span {
    span.to_usize_range()
}

fn qualify(session: &Session, module: &ModuleName, name: Symbol) -> String {
    format!("{}.{}", module.as_string(session), session.str(name))
}

fn module_prefix(session: &Session, segments: &[Symbol]) -> String {
    segments
        .iter()
        .map(|segment| session.str(*segment))
        .collect::<Vec<_>>()
        .join(".")
}

fn resolver_trace_enabled() -> bool {
    match env::var("SOLVEIG_RESOLVER_TRACE") {
        Ok(value) => {
            let value = value.trim().to_ascii_lowercase();
            !value.is_empty() && value != "0" && value != "false" && value != "off"
        }
        Err(_) => false,
    }
}

fn trace(session: &Session, module: &ModuleName, message: impl AsRef<str>) {
    if resolver_trace_enabled() {
        eprintln!(
            "[resolver:{}] {}",
            module.as_string(session),
            message.as_ref()
        );
    }
}

fn trace_predicates(session: &Session, module: &ModuleName, label: &str, predicates: &[Predicate]) {
    if resolver_trace_enabled() && !predicates.is_empty() {
        let rendered = predicates
            .iter()
            .map(|p| format!("{:?}", p))
            .collect::<Vec<_>>()
            .join(", ");

        trace(session, module, format!("{label}: {rendered}"));
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResolverImports {
    pub tycon_kinds: HashMap<String, Kind>,
    pub class_param_kinds: HashMap<String, Vec<Kind>>,
}

#[derive(Debug)]
struct LocalResolverEnv {
    /// Surface class name -> canonical class name for classes defined in this module.
    ///
    /// This is deliberately separate from `ModuleContext.local_api`: while resolving a
    /// module, class predicates in signatures/methods/instances must canonicalize
    /// against the source declarations currently being resolved. Relying only on the
    /// already-built API made local class names like `Functor` leak through as raw
    /// predicate names in some paths.
    classes: HashMap<Symbol, String>,
}

impl LocalResolverEnv {
    fn from_parsed_module(
        session: &Session,
        module_name: &ModuleName,
        parsed: &ParsedModule,
    ) -> Self {
        let classes = parsed
            .defs
            .iter()
            .filter_map(|defn| match &defn.0 {
                SurfaceDefinition::ClassDef { name, .. } => {
                    Some((name.0, qualify(session, module_name, name.0)))
                }
                _ => None,
            })
            .collect();

        Self { classes }
    }
}

pub struct Resolver<'a> {
    session: &'a Session,
}

impl<'a> Resolver<'a> {
    pub fn new(session: &'a Session) -> Self {
        Self { session }
    }

    fn symbol_text(&self, symbol: Symbol) -> &'static str {
        self.session.str(symbol)
    }

    fn legacy_qualified_ref(&self, r: &NameRef) -> String {
        let module = module_prefix(self.session, r.prefix());
        format!("{}.{}", module, r.last_text(self.session))
    }

    fn resolve_term_ref(&self, r: &NameRef, cx: &ModuleContext) -> String {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.terms.get(name) {
                    return sym.display(self.session);
                }

                if let Some(sym) = cx.local_api.terms.get(name) {
                    return sym.display(self.session);
                }

                let mut found: Option<String> = None;

                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.terms.get(name) {
                        let display = sym.display(self.session);

                        if let Some(prev) = &found {
                            if prev != &display {
                                panic!("ambiguous imported term '{}'", self.symbol_text(*name));
                            }
                        } else {
                            found = Some(display);
                        }
                    }
                }

                found.unwrap_or_else(|| self.symbol_text(*name).to_string())
            }

            [first, rest @ ..] => {
                if let Some(imp) = cx.imported_apis.iter().find(|i| i.alias == *first) {
                    let local_name = *rest
                        .last()
                        .expect("qualified name must have a last segment");

                    if let Some(sym) = imp.api.terms.get(&local_name) {
                        return sym.display(self.session);
                    }

                    panic!(
                        "unknown imported term {}.{}",
                        self.symbol_text(*first),
                        self.symbol_text(local_name)
                    );
                }

                self.legacy_qualified_ref(r)
            }

            [] => panic!("ICE: empty term name ref"),
        }
    }

    fn resolve_tycon_ref(&self, r: &NameRef, cx: &ModuleContext) -> String {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.types.get(name) {
                    return sym.display(self.session);
                }

                if let Some(sym) = cx.local_api.types.get(name) {
                    return sym.display(self.session);
                }

                let mut found: Option<String> = None;

                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.types.get(name) {
                        let display = sym.display(self.session);

                        if let Some(prev) = &found {
                            if prev != &display {
                                panic!("ambiguous imported type '{}'", self.symbol_text(*name));
                            }
                        } else {
                            found = Some(display);
                        }
                    }
                }

                found.unwrap_or_else(|| self.symbol_text(*name).to_string())
            }

            [first, rest @ ..] => {
                if let Some(imp) = cx.imported_apis.iter().find(|i| i.alias == *first) {
                    let local_name = *rest
                        .last()
                        .expect("qualified name must have a last segment");

                    if let Some(sym) = imp.api.types.get(&local_name) {
                        return sym.display(self.session);
                    }

                    panic!(
                        "unknown imported type {}.{}",
                        self.symbol_text(*first),
                        self.symbol_text(local_name)
                    );
                }

                self.legacy_qualified_ref(r)
            }

            [] => panic!("ICE: empty type constructor name ref"),
        }
    }

    fn resolve_ctor_ref(&self, r: &NameRef, cx: &ModuleContext) -> String {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.constructors.get(name) {
                    return sym.display(self.session);
                }

                if let Some(sym) = cx.local_api.constructors.get(name) {
                    return sym.display(self.session);
                }

                let mut found: Option<String> = None;

                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.constructors.get(name) {
                        let display = sym.display(self.session);

                        if let Some(prev) = &found {
                            if prev != &display {
                                panic!(
                                    "ambiguous imported constructor '{}'",
                                    self.symbol_text(*name)
                                );
                            }
                        } else {
                            found = Some(display);
                        }
                    }
                }

                found.unwrap_or_else(|| self.symbol_text(*name).to_string())
            }

            [first, rest @ ..] => {
                if let Some(imp) = cx.imported_apis.iter().find(|i| i.alias == *first) {
                    let local_name = *rest
                        .last()
                        .expect("qualified name must have a last segment");

                    if let Some(sym) = imp.api.constructors.get(&local_name) {
                        return sym.display(self.session);
                    }

                    panic!(
                        "unknown imported constructor {}.{}",
                        self.symbol_text(*first),
                        self.symbol_text(local_name)
                    );
                }

                self.legacy_qualified_ref(r)
            }

            [] => panic!("ICE: empty constructor name ref"),
        }
    }

    fn resolve_class_ref(
        &self,
        r: &NameRef,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> String {
        match r.segments.as_slice() {
            [name] => self.resolve_unqualified_class_name(name, cx, local),

            [first, rest @ ..] => {
                if let Some(imp) = cx.imported_apis.iter().find(|i| i.alias == *first) {
                    let local_name = *rest
                        .last()
                        .expect("qualified name must have a last segment");

                    if let Some(sym) = imp.api.classes.get(&local_name) {
                        return sym.display(self.session);
                    }

                    panic!(
                        "unknown imported class {}.{}",
                        self.symbol_text(*first),
                        self.symbol_text(local_name)
                    );
                }

                self.legacy_qualified_ref(r)
            }

            [] => panic!("ICE: empty class name ref"),
        }
    }

    fn resolve_unqualified_class_name(
        &self,
        name: &Symbol,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> String {
        if let Some(qualified) = local.classes.get(name) {
            return qualified.clone();
        }

        if let Some(sym) = cx.local_api.classes.get(name) {
            return sym.display(self.session);
        }

        let mut found: Option<String> = None;

        for imp in &cx.imported_apis {
            if let Some(sym) = imp.api.classes.get(name) {
                let display = sym.display(self.session);

                if let Some(prev) = &found {
                    if prev != &display {
                        panic!("ambiguous imported class '{}'", self.symbol_text(*name));
                    }
                } else {
                    found = Some(display);
                }
            }
        }

        found.unwrap_or_else(|| self.symbol_text(*name).to_string())
    }

    fn canonicalize_predicate(
        &self,
        pred: Predicate,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Predicate {
        let class_name = if pred.class_name.contains('.') {
            pred.class_name
        } else {
            let name = self.session.intern(&pred.class_name);
            self.resolve_unqualified_class_name(&name, cx, local)
        };

        Predicate {
            class_name,
            args: pred.args,
        }
    }

    fn canonicalize_scheme_classes(
        &self,
        scheme: Scheme,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Scheme {
        Scheme {
            quantifiers: scheme.quantifiers,
            predicates: scheme
                .predicates
                .into_iter()
                .map(|pred| self.canonicalize_predicate(pred, cx, local))
                .collect(),
            ty: scheme.ty,
        }
    }

    fn class_param_kind_env(
        imported: &ResolverImports,
        class_defs: &[ClassDefinition],
    ) -> HashMap<String, Vec<Kind>> {
        let mut class_env = imported.class_param_kinds.clone();

        class_env.extend(class_defs.iter().map(|class_def| {
            (
                class_def.name.clone(),
                class_def
                    .params
                    .iter()
                    .map(|(_, kind)| kind.clone())
                    .collect::<Vec<_>>(),
            )
        }));

        class_env
    }

    fn kind_check_instance_predicates(
        &self,
        context: Vec<Predicate>,
        head: Predicate,
        all_tycon_kinds: &HashMap<String, Kind>,
        class_env: &HashMap<String, Vec<Kind>>,
    ) -> Result<(Vec<Predicate>, Predicate), KindError> {
        let context_len = context.len();

        let mut predicates = context;
        predicates.push(head);

        // Reuse signature kind inference as a predicate-kind checker.
        //
        // It already knows how to:
        // - create implicit kinded quantifiers for free type variables in predicates;
        // - look up class parameter kinds;
        // - reject ill-kinded heads like `Functor Int` because `Int : *`, but
        //   `Functor` expects an argument of kind `* -> *`.
        let pseudo_scheme = Scheme {
            quantifiers: Vec::new(),
            predicates,
            ty: TypeExpr::TyUnit,
        };

        let info = SignatureKindInferencer::with_envs(all_tycon_kinds.clone(), class_env.clone())
            .infer_scheme(&pseudo_scheme)?;

        let mut predicates = info.scheme.predicates;
        let checked_head = predicates
            .pop()
            .expect("instance kind checking lost instance head predicate");

        debug_assert_eq!(
            predicates.len(),
            context_len,
            "instance kind checking changed predicate count unexpectedly"
        );

        Ok((predicates, checked_head))
    }

    fn resolve_surface_predicate(
        &self,
        pred: &SpannedWrapper<SurfacePredicate>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Predicate {
        let resolved = Predicate {
            class_name: self.resolve_class_ref(&pred.0.class_name.0, cx, local),
            args: pred
                .0
                .args
                .iter()
                .map(|arg| self.resolve_surface_type_in_module(arg, cx, local))
                .collect(),
        };

        self.canonicalize_predicate(resolved, cx, local)
    }

    fn resolve_surface_scheme(
        &self,
        ty: &SpannedWrapper<SurfaceTy>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Scheme {
        let raw = self.resolve_surface_type_in_module(ty, cx, local);
        let (quantifiers, predicates, body_ty) = raw.peel_scheme_like();

        self.canonicalize_scheme_classes(
            Scheme {
                quantifiers,
                predicates,
                ty: body_ty,
            },
            cx,
            local,
        )
    }

    fn resolve_surface_type_in_module(
        &self,
        sf: &SpannedWrapper<SurfaceTy>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> TypeExpr {
        match &sf.0 {
            SurfaceTy::Var(var_name) => TypeExpr::make_var(self.symbol_text(*var_name).to_string()),

            SurfaceTy::Arr(lhs, rhs) => TypeExpr::make_arr(
                self.resolve_surface_type_in_module(lhs, cx, local),
                self.resolve_surface_type_in_module(rhs, cx, local),
            ),

            SurfaceTy::ForAll(quants, body) => quants.iter().rev().fold(
                self.resolve_surface_type_in_module(body, cx, local),
                |acc, quant| {
                    TypeExpr::make_forall(self.symbol_text(quant.0).to_string(), Kind::Star, acc)
                },
            ),

            SurfaceTy::Qualified(preds, body) => TypeExpr::make_qualified(
                preds
                    .iter()
                    .map(|pred| self.resolve_surface_predicate(pred, cx, local))
                    .collect(),
                self.resolve_surface_type_in_module(body, cx, local),
            ),

            SurfaceTy::Unit => TypeExpr::TyUnit,

            SurfaceTy::Con(con) => TypeExpr::TyCon(self.resolve_tycon_ref(&con.0, cx)),

            SurfaceTy::App(lhs, rhs) => TypeExpr::make_app(
                self.resolve_surface_type_in_module(lhs, cx, local),
                self.resolve_surface_type_in_module(rhs, cx, local),
            ),
        }
    }

    fn resolve_surface_pattern_in_module(
        &self,
        sf: &SpannedWrapper<SurfacePattern>,
        cx: &ModuleContext,
    ) -> PatternNode<Span> {
        let span = to_core_span(sf.1);

        let node = match &sf.0 {
            SurfacePattern::Var(var_name) => Pattern::Var(self.symbol_text(*var_name).to_string()),
            SurfacePattern::Lit(lit) => {
                let core_lit = match lit {
                    SurfaceLiteral::Int(int_lit_value) => Literal::Int(*int_lit_value),
                };

                Pattern::Lit(core_lit)
            }
            SurfacePattern::Wild => Pattern::Wild,
            SurfacePattern::Con(con_name, args) => Pattern::Con(
                self.resolve_ctor_ref(&con_name.0, cx),
                args.iter()
                    .map(|arg| self.resolve_surface_pattern_in_module(arg, cx))
                    .collect(),
            ),
        };

        PatternNode::new(node, span)
    }

    fn collect_surface_pattern_binders(
        pat: &SpannedWrapper<SurfacePattern>,
        out: &mut HashSet<Symbol>,
    ) {
        match &pat.0 {
            SurfacePattern::Var(name) => {
                out.insert(*name);
            }
            SurfacePattern::Lit(_) => {}
            SurfacePattern::Wild => {}
            SurfacePattern::Con(_, args) => {
                for arg in args {
                    Self::collect_surface_pattern_binders(arg, out);
                }
            }
        }
    }

    fn resolve_term_ref_with_binders(
        &self,
        r: &NameRef,
        cx: &ModuleContext,
        bound_terms: &HashSet<Symbol>,
    ) -> String {
        match r.segments.as_slice() {
            [name] if bound_terms.contains(name) => self.symbol_text(*name).to_string(),
            _ => self.resolve_term_ref(r, cx),
        }
    }

    fn resolve_surface_expr_in_module(
        &mut self,
        sf: &SpannedWrapper<SurfaceExpr>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        bound_terms: &HashSet<Symbol>,
    ) -> TermExprNode<Span> {
        let span = to_core_span(sf.1);

        let node = match &sf.0 {
            SurfaceExpr::Unit => TermExpr::Unit,
            SurfaceExpr::Hole => TermExpr::Hole,
            SurfaceExpr::Var(var_name) => {
                TermExpr::Var(self.resolve_term_ref_with_binders(&var_name.0, cx, bound_terms))
            }
            SurfaceExpr::Con(con_name) => TermExpr::Con(self.resolve_ctor_ref(&con_name.0, cx)),

            SurfaceExpr::Literal(lit) => match lit {
                SurfaceLiteral::Int(value) => TermExpr::Lit(Literal::Int(*value)),
            },

            SurfaceExpr::App(lhs, rhs) => TermExpr::App(
                Box::new(self.resolve_surface_expr_in_module(lhs, cx, local, bound_terms)),
                Box::new(self.resolve_surface_expr_in_module(rhs, cx, local, bound_terms)),
            ),

            SurfaceExpr::Ann(expr, ty) => TermExpr::Anno(
                Box::new(self.resolve_surface_expr_in_module(expr, cx, local, bound_terms)),
                self.resolve_surface_type_in_module(ty, cx, local),
            ),

            SurfaceExpr::Abs(params, body) => {
                let mut body_bound_terms = bound_terms.clone();

                for (param_name, _) in params {
                    body_bound_terms.insert(param_name.0);
                }

                let mut body_node =
                    self.resolve_surface_expr_in_module(body, cx, local, &body_bound_terms);

                for (param_name, param_ty_anno) in params.iter().rev() {
                    let param_ty = param_ty_anno
                        .as_ref()
                        .map(|ty| self.resolve_surface_type_in_module(ty, cx, local));

                    body_node = TermExprNode::new(
                        TermExpr::Abs(
                            self.symbol_text(param_name.0).to_string(),
                            param_ty,
                            Box::new(body_node),
                        ),
                        span.clone(),
                    );
                }

                return body_node;
            }

            SurfaceExpr::IfThenElse(cond, then_branch, else_branch) => TermExpr::IfThenElse(
                Box::new(self.resolve_surface_expr_in_module(cond, cx, local, bound_terms)),
                Box::new(self.resolve_surface_expr_in_module(then_branch, cx, local, bound_terms)),
                Box::new(self.resolve_surface_expr_in_module(else_branch, cx, local, bound_terms)),
            ),

            SurfaceExpr::Case(scrutinee, arms) => TermExpr::Case(
                Box::new(self.resolve_surface_expr_in_module(scrutinee, cx, local, bound_terms)),
                arms.iter()
                    .map(|arm| self.resolve_surface_case_arm_in_module(arm, cx, local, bound_terms))
                    .collect(),
            ),

            SurfaceExpr::BinaryOperator(op, lhs, rhs) => {
                let core_binop = match op {
                    SurfaceBinOp::Plus => BinOp::Plus,
                    SurfaceBinOp::Minus => BinOp::Minus,
                    SurfaceBinOp::Mult => BinOp::Mult,
                    SurfaceBinOp::Div => BinOp::Div,
                    SurfaceBinOp::Mod => BinOp::Mod,
                    SurfaceBinOp::Less => BinOp::Less,
                    SurfaceBinOp::Greater => BinOp::Greater,
                    SurfaceBinOp::LessOrEqual => BinOp::LessOrEqual,
                    SurfaceBinOp::GreaterOrEqual => BinOp::GreaterOrEqual,
                    SurfaceBinOp::Equal => BinOp::Equal,
                };

                TermExpr::BinaryOperator(
                    core_binop,
                    Box::new(self.resolve_surface_expr_in_module(lhs, cx, local, bound_terms)),
                    Box::new(self.resolve_surface_expr_in_module(rhs, cx, local, bound_terms)),
                )
            }

            SurfaceExpr::UnaryOperator(op, rhs) => {
                let core_unop = match op {
                    SurfaceUnOp::Neg => UnOp::Neg,
                };

                TermExpr::UnaryOperator(
                    core_unop,
                    Box::new(self.resolve_surface_expr_in_module(rhs, cx, local, bound_terms)),
                )
            }

            SurfaceExpr::Let(binder_name, binder_expr, in_expr) => {
                let resolved_binder =
                    self.resolve_surface_expr_in_module(binder_expr, cx, local, bound_terms);

                let mut body_bound_terms = bound_terms.clone();
                body_bound_terms.insert(binder_name.0);

                let resolved_in_expr =
                    self.resolve_surface_expr_in_module(in_expr, cx, local, &body_bound_terms);

                TermExpr::Let(
                    self.symbol_text(binder_name.0).to_string(),
                    Box::new(resolved_binder),
                    Box::new(resolved_in_expr),
                )
            }
        };

        TermExprNode::new(node, span)
    }

    fn resolve_surface_case_arm_in_module(
        &mut self,
        sf: &SpannedWrapper<SurfaceMatchArm>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        bound_terms: &HashSet<Symbol>,
    ) -> CaseArmNode<Span> {
        let pattern = self.resolve_surface_pattern_in_module(&sf.0.pattern, cx);

        let mut arm_bound_terms = bound_terms.clone();
        Self::collect_surface_pattern_binders(&sf.0.pattern, &mut arm_bound_terms);

        CaseArmNode::new(
            CaseArm {
                pattern,
                expr: self.resolve_surface_expr_in_module(&sf.0.expr, cx, local, &arm_bound_terms),
            },
            to_core_span(sf.1),
        )
    }

    fn peel_arrow<'ty>(&self, ty: &'ty TypeExpr) -> Option<(&'ty TypeExpr, &'ty TypeExpr)> {
        match ty {
            TypeExpr::TyArr(lhs, rhs) => Some((lhs, rhs)),
            _ => None,
        }
    }

    fn wrap_function_body_without_signature_in_module(
        &mut self,
        body: &SpannedWrapper<SurfaceExpr>,
        params: &[SpannedWrapper<Symbol>],
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> TermExprNode<Span> {
        let span = to_core_span(body.1);
        let mut bound_terms = HashSet::new();

        for param in params {
            bound_terms.insert(param.0);
        }

        let mut body_node = self.resolve_surface_expr_in_module(body, cx, local, &bound_terms);

        for param in params.iter().rev() {
            body_node = TermExprNode::new(
                TermExpr::Abs(
                    self.symbol_text(param.0).to_string(),
                    None,
                    Box::new(body_node),
                ),
                span.clone(),
            );
        }

        body_node
    }

    fn wrap_function_body_with_signature_in_module(
        &mut self,
        sig: &Scheme,
        body: &SpannedWrapper<SurfaceExpr>,
        params: &[SpannedWrapper<Symbol>],
        name: Symbol,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> TermExprNode<Span> {
        let span = to_core_span(body.1);
        let mut bound_terms = HashSet::new();

        for param in params {
            bound_terms.insert(param.0);
        }

        let mut body_node = self.resolve_surface_expr_in_module(body, cx, local, &bound_terms);

        let mut remaining = &sig.ty;
        let mut annos: Vec<(String, TypeExpr)> = Vec::new();

        for param in params {
            let (arg_ty, ret_ty) = self.peel_arrow(remaining).unwrap_or_else(|| {
                panic!(
                    "parameter mismatch for '{}'. The number of parameters doesn't match the function signature",
                    self.symbol_text(name)
                )
            });

            annos.push((self.symbol_text(param.0).to_string(), arg_ty.clone()));
            remaining = ret_ty;
        }

        for (param_name, param_ty) in annos.into_iter().rev() {
            body_node = TermExprNode::new(
                TermExpr::Abs(param_name, Some(param_ty), Box::new(body_node)),
                span.clone(),
            );
        }

        for (quant_name, quant_kind) in sig.quantifiers.clone().into_iter().rev() {
            body_node = TermExprNode::new(
                TermExpr::TyAbs(quant_name, quant_kind, Box::new(body_node)),
                span.clone(),
            );
        }

        body_node
    }

    pub fn resolve_module(
        mut self,
        module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        imported: &ResolverImports,
    ) -> Result<ResolvedModule<Span>, KindError> {
        let local = LocalResolverEnv::from_parsed_module(self.session, module_name, parsed);

        trace(
            self.session,
            module_name,
            format!(
                "start: defs={}, imported_tycons={}, imported_classes={}, local_classes={:?}",
                parsed.defs.len(),
                imported.tycon_kinds.len(),
                imported.class_param_kinds.len(),
                local.classes
            ),
        );

        let raw_data_defs = self.collect_raw_data_defs(module_name, parsed, cx, &local);

        let (data_defs, local_tycon_kinds, constructor_types, all_tycon_kinds) =
            self.resolve_data_defs(module_name, raw_data_defs, imported)?;

        let class_defs =
            self.resolve_class_defs(module_name, parsed, cx, imported, &local, &all_tycon_kinds)?;

        let class_env = Self::class_param_kind_env(imported, &class_defs);

        let instance_defs = self.resolve_instance_defs(
            module_name,
            parsed,
            cx,
            &local,
            &all_tycon_kinds,
            &class_env,
        )?;

        let func_schemes = self.resolve_signatures(
            module_name,
            parsed,
            cx,
            imported,
            &local,
            &local_tycon_kinds,
            &class_defs,
        )?;

        let func_defs = self.resolve_function_defs(module_name, parsed, cx, &local, &func_schemes);

        trace(
            self.session,
            module_name,
            format!(
                "done: data={}, classes={}, instances={}, signatures={}, functions={}",
                data_defs.len(),
                class_defs.len(),
                instance_defs.len(),
                func_schemes.len(),
                func_defs.len()
            ),
        );

        Ok(ResolvedModule {
            data_defs,
            class_defs,
            instance_defs,
            tycon_kinds: local_tycon_kinds,
            constructor_types,
            func_schemes,
            func_defs,
        })
    }

    fn collect_raw_data_defs(
        &self,
        module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Vec<RawDataDef> {
        parsed
            .defs
            .iter()
            .filter_map(|defn| match &defn.0 {
                SurfaceDefinition::DataDef {
                    name: spanned_name,
                    ty_params,
                    constructors,
                } => {
                    let (name, _span) = spanned_name;

                    Some(RawDataDef {
                        name: qualify(self.session, module_name, *name),
                        ty_params: ty_params
                            .iter()
                            .map(|p| self.symbol_text(p.0).to_string())
                            .collect(),
                        constructors: constructors
                            .iter()
                            .map(|constructor| RawConstructorDef {
                                name: qualify(self.session, module_name, constructor.0.name.0),
                                fields: constructor
                                    .0
                                    .params
                                    .iter()
                                    .map(|ty| self.resolve_surface_type_in_module(ty, cx, local))
                                    .collect(),
                            })
                            .collect(),
                    })
                }

                _ => None,
            })
            .collect()
    }

    fn resolve_data_defs(
        &self,
        module_name: &ModuleName,
        raw_data_defs: Vec<RawDataDef>,
        imported: &ResolverImports,
    ) -> Result<
        (
            Vec<DataDefinition>,
            HashMap<String, Kind>,
            HashMap<String, TypeExpr>,
            HashMap<String, Kind>,
        ),
        KindError,
    > {
        let (inferred_data, all_tycon_kinds) =
            infer_all_data_kinds_with_env(raw_data_defs, imported.tycon_kinds.clone())?;

        let mut data_defs = Vec::new();
        let mut local_tycon_kinds = HashMap::new();
        let mut constructor_types = HashMap::new();

        for idef in &inferred_data {
            let mut core_ctors = Vec::new();

            for ctor in &idef.constructors {
                constructor_types.insert(ctor.name.clone(), ctor.full_type.clone());

                core_ctors.push(ConstructorDef {
                    name: ctor.name.clone(),
                    fields: ctor.kinded_fields.clone(),
                    full_type: ctor.full_type.clone(),
                });
            }

            local_tycon_kinds.insert(idef.name.clone(), idef.tycon_kind.clone());

            data_defs.push(DataDefinition {
                name: idef.name.clone(),
                ty_params: idef.ty_params.clone(),
                tycon_kind: idef.tycon_kind.clone(),
                constructors: core_ctors,
            });
        }

        trace(
            self.session,
            module_name,
            format!(
                "data: local_tycons={:?}, local_ctors={:?}",
                local_tycon_kinds.keys().collect::<Vec<_>>(),
                constructor_types.keys().collect::<Vec<_>>()
            ),
        );

        Ok((
            data_defs,
            local_tycon_kinds,
            constructor_types,
            all_tycon_kinds,
        ))
    }

    fn resolve_class_defs(
        &self,
        module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        imported: &ResolverImports,
        local: &LocalResolverEnv,
        all_tycon_kinds: &HashMap<String, Kind>,
    ) -> Result<Vec<ClassDefinition>, KindError> {
        let raw_class_defs = parsed
            .defs
            .iter()
            .filter_map(|defn| match &defn.0 {
                SurfaceDefinition::ClassDef {
                    name,
                    ty_params,
                    superclasses,
                    methods,
                } => {
                    let class_name = qualify(self.session, module_name, name.0);

                    let class_params = ty_params
                        .iter()
                        .map(|param| (self.symbol_text(param.0).to_string(), Kind::Star))
                        .collect::<Vec<_>>();

                    let class_head_pred = Predicate {
                        class_name: class_name.clone(),
                        args: ty_params
                            .iter()
                            .map(|param| TypeExpr::TyVar(self.symbol_text(param.0).to_string()))
                            .collect(),
                    };

                    let resolved_superclasses = superclasses
                        .iter()
                        .map(|pred| self.resolve_surface_predicate(pred, cx, local))
                        .collect::<Vec<_>>();

                    trace_predicates(
                        self.session,
                        module_name,
                        &format!("class {} superclasses", class_name),
                        &resolved_superclasses,
                    );

                    let resolved_methods = methods
                        .iter()
                        .map(|method| {
                            let raw_method_scheme =
                                self.resolve_surface_scheme(&method.ty, cx, local);

                            let mut predicates =
                                Vec::with_capacity(1 + raw_method_scheme.predicates.len());
                            predicates.push(class_head_pred.clone());
                            predicates.extend(raw_method_scheme.predicates.clone());

                            let scheme = Scheme {
                                quantifiers: raw_method_scheme.quantifiers.clone(),
                                predicates,
                                ty: raw_method_scheme.ty,
                            };

                            trace_predicates(
                                self.session,
                                module_name,
                                &format!(
                                    "class method {}.{} predicates",
                                    class_name,
                                    self.symbol_text(method.name.0)
                                ),
                                &scheme.predicates,
                            );

                            ClassMethodDef {
                                name: qualify(self.session, module_name, method.name.0),
                                scheme,
                            }
                        })
                        .collect::<Vec<_>>();

                    Some(ClassDefinition {
                        name: class_name,
                        params: class_params,
                        superclasses: resolved_superclasses,
                        methods: resolved_methods,
                    })
                }

                _ => None,
            })
            .collect::<Vec<_>>();

        let mut seed_class_env = imported.class_param_kinds.clone();

        seed_class_env.extend(raw_class_defs.iter().map(|class_def| {
            (
                class_def.name.clone(),
                class_def
                    .params
                    .iter()
                    .map(|(_, _)| Kind::make_var("?seed".to_string()))
                    .collect::<Vec<_>>(),
            )
        }));

        let kinded_class_defs = raw_class_defs
            .iter()
            .map(|class_def| {
                ClassKindInferencer::new(all_tycon_kinds.clone(), seed_class_env.clone())
                    .infer_class(class_def)
            })
            .collect::<Result<Vec<_>, _>>()?;

        trace(
            self.session,
            module_name,
            format!(
                "classes: {:?}",
                kinded_class_defs
                    .iter()
                    .map(|class_def| (&class_def.name, &class_def.params))
                    .collect::<Vec<_>>()
            ),
        );

        Ok(kinded_class_defs)
    }

    fn resolve_instance_defs(
        &mut self,
        module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        all_tycon_kinds: &HashMap<String, Kind>,
        class_env: &HashMap<String, Vec<Kind>>,
    ) -> Result<Vec<InstanceDefinition<Span>>, KindError> {
        let mut instance_defs = Vec::new();

        for defn in parsed.defs.iter() {
            let SurfaceDefinition::InstanceDef {
                context,
                class_name,
                args,
                methods,
            } = &defn.0
            else {
                continue;
            };

            let raw_head = Predicate {
                class_name: self.resolve_class_ref(
                    &NameRef {
                        segments: vec![class_name.0],
                    },
                    cx,
                    local,
                ),
                args: args
                    .iter()
                    .map(|arg| self.resolve_surface_type_in_module(arg, cx, local))
                    .collect(),
            };

            let raw_head = self.canonicalize_predicate(raw_head, cx, local);

            let raw_context = context
                .iter()
                .map(|pred| self.resolve_surface_predicate(pred, cx, local))
                .collect::<Vec<_>>();

            trace_predicates(
                self.session,
                module_name,
                "instance raw context",
                &raw_context,
            );
            trace_predicates(
                self.session,
                module_name,
                "instance raw head",
                std::slice::from_ref(&raw_head),
            );

            let (checked_context, checked_head) = self.kind_check_instance_predicates(
                raw_context,
                raw_head,
                all_tycon_kinds,
                class_env,
            )?;

            trace_predicates(
                self.session,
                module_name,
                "instance checked context",
                &checked_context,
            );
            trace_predicates(
                self.session,
                module_name,
                "instance checked head",
                std::slice::from_ref(&checked_head),
            );

            let resolved_methods = methods
                .iter()
                .map(|method| InstanceMethodDef {
                    name: self.symbol_text(method.name.0).to_string(),
                    body: self.wrap_function_body_without_signature_in_module(
                        &method.body,
                        &method.params,
                        cx,
                        local,
                    ),
                })
                .collect::<Vec<_>>();

            instance_defs.push(InstanceDefinition {
                context: checked_context,
                head: checked_head,
                methods: resolved_methods,
            });
        }

        Ok(instance_defs)
    }

    fn resolve_signatures(
        &self,
        module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        imported: &ResolverImports,
        local: &LocalResolverEnv,
        local_tycon_kinds: &HashMap<String, Kind>,
        class_defs: &[ClassDefinition],
    ) -> Result<HashMap<String, Scheme>, KindError> {
        let mut func_schemes = HashMap::new();

        let mut sig_tycon_env = imported.tycon_kinds.clone();
        sig_tycon_env.extend(local_tycon_kinds.clone());

        let sig_class_env = Self::class_param_kind_env(imported, class_defs);

        for defn in parsed.defs.iter() {
            let SurfaceDefinition::SigDef {
                name: spanned_name,
                ty,
            } = &defn.0
            else {
                continue;
            };

            let (name, _span) = spanned_name;
            let fn_name = qualify(self.session, module_name, *name);
            let raw_scheme = self.resolve_surface_scheme(ty, cx, local);

            trace_predicates(
                self.session,
                module_name,
                &format!("signature {} predicates", fn_name),
                &raw_scheme.predicates,
            );

            let inf =
                SignatureKindInferencer::with_envs(sig_tycon_env.clone(), sig_class_env.clone());
            let info = inf.infer_scheme(&raw_scheme)?;

            func_schemes.insert(fn_name, info.scheme.clone());
        }

        Ok(func_schemes)
    }

    fn resolve_function_defs(
        &mut self,
        module_name: &ModuleName,
        parsed: &ParsedModule,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
        func_schemes: &HashMap<String, Scheme>,
    ) -> Vec<TermDefinition<Span>> {
        parsed
            .defs
            .iter()
            .filter_map(|defn| match &defn.0 {
                SurfaceDefinition::FunDef {
                    name: spanned_name,
                    body,
                    params,
                } => {
                    let (name, _span) = spanned_name;
                    let fn_name = qualify(self.session, module_name, *name);
                    let sig = func_schemes.get(&fn_name).cloned();

                    let wrapped = match &sig {
                        Some(sig) => self.wrap_function_body_with_signature_in_module(
                            sig, body, params, *name, cx, local,
                        ),
                        None => self.wrap_function_body_without_signature_in_module(
                            body, params, cx, local,
                        ),
                    };

                    Some(TermDefinition {
                        name: fn_name,
                        signature: sig,
                        body: wrapped,
                    })
                }

                _ => None,
            })
            .collect()
    }
}
