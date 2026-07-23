use std::collections::{HashMap, HashSet};
use std::env;
use std::ops::Range;

use base::{FileSpan, Session, Span as SourceSpan, Symbol};
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
use syntax::data_kind_infer::{RawConstructorDef, RawDataDef, infer_all_data_kinds_with_env};
use syntax::inference::class_kind_infer::ClassKindInferencer;
use syntax::signature_infer::{KindError, SignatureKindInferencer};

use crate::project::{
    DefId, ModuleContext, ModuleTable, ResolveError, ResolveNameKind, defid_to_legacy_string,
};

type Span = Range<usize>;

fn to_core_span(span: SourceSpan) -> Span {
    span.to_usize_range()
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
pub struct ResolvedModuleInfo<A> {
    pub module: ResolvedModule<A>,
    pub resolutions: Vec<(FileSpan, DefId)>,
}

#[derive(Debug)]
pub enum ResolverError {
    Resolve(Vec<ResolveError>),
    Kind(KindError),
}

impl From<KindError> for ResolverError {
    fn from(error: KindError) -> Self {
        Self::Kind(error)
    }
}

#[derive(Debug)]
struct LocalResolverEnv {
    /// Surface class name -> canonical class id for classes defined in this module.
    ///
    /// This is deliberately separate from `ModuleContext.local_api`: while resolving a
    /// module, class predicates in signatures/methods/instances must canonicalize
    /// against the source declarations currently being resolved. Relying only on the
    /// already-built API made local class names like `Functor` leak through as raw
    /// predicate names in some paths.
    classes: HashMap<Symbol, DefId>,
}

impl LocalResolverEnv {
    fn from_parsed_module(module_id: crate::project::ModuleId, parsed: &ParsedModule) -> Self {
        let classes = parsed
            .defs
            .iter()
            .filter_map(|defn| match &defn.0 {
                SurfaceDefinition::ClassDef { name, .. } => Some((
                    name.0,
                    DefId {
                        module: module_id,
                        name: name.0,
                    },
                )),
                _ => None,
            })
            .collect();

        Self { classes }
    }
}

pub struct Resolver<'a> {
    session: &'a Session,
    modules: &'a ModuleTable,
    errors: Vec<ResolveError>,
    resolutions: Vec<(FileSpan, DefId)>,
}

impl<'a> Resolver<'a> {
    pub fn new(session: &'a Session, modules: &'a ModuleTable) -> Self {
        Self {
            session,
            modules,
            errors: Vec::new(),
            resolutions: Vec::new(),
        }
    }

    fn symbol_text(&self, symbol: Symbol) -> &'static str {
        self.session.str(symbol)
    }

    fn file_span(&self, cx: &ModuleContext, span: SourceSpan) -> FileSpan {
        FileSpan::new(cx.file, span)
    }

    fn record_resolution(&mut self, span: FileSpan, def_id: DefId) {
        self.resolutions.push((span, def_id));
    }

    fn legacy_name(&self, def_id: DefId) -> String {
        defid_to_legacy_string(self.session, self.modules, def_id)
    }

    fn local_def_id(&self, cx: &ModuleContext, name: Symbol) -> DefId {
        DefId {
            module: cx.module_id,
            name,
        }
    }

    fn unresolved_name(&self, r: &NameRef) -> String {
        r.display(self.session)
    }

    fn imported_by_alias_or_module<'cx>(
        &self,
        r: &NameRef,
        cx: &'cx ModuleContext,
    ) -> Option<&'cx crate::project::ImportedApi> {
        let [first, ..] = r.segments.as_slice() else {
            return None;
        };

        cx.imported_apis
            .iter()
            .find(|import| import.alias == *first)
            .or_else(|| {
                cx.imported_apis
                    .iter()
                    .find(|import| import.module.0.as_slice() == r.prefix())
            })
    }

    fn report_unknown_name(
        &mut self,
        span: FileSpan,
        kind: ResolveNameKind,
        name: impl Into<String>,
    ) {
        self.errors.push(ResolveError::UnknownName {
            span,
            kind,
            name: name.into(),
        });
    }

    fn report_ambiguous_import(
        &mut self,
        span: FileSpan,
        kind: ResolveNameKind,
        name: Symbol,
        candidates: Vec<DefId>,
    ) {
        self.errors.push(ResolveError::AmbiguousImport {
            span,
            kind,
            name,
            candidates,
        });
    }

    fn report_unknown_module(&mut self, span: FileSpan, module: ModuleName) {
        self.errors
            .push(ResolveError::UnknownModule { span, module });
    }

    fn take_errors(&mut self) -> Option<Vec<ResolveError>> {
        if self.errors.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.errors))
        }
    }

    fn resolve_term_ref(&mut self, r: &NameRef, span: FileSpan, cx: &ModuleContext) -> String {
        match self.resolve_term_defid(r, span, cx) {
            Some(def_id) => {
                self.record_resolution(span, def_id);
                self.legacy_name(def_id)
            }
            None => self.unresolved_name(r),
        }
    }

    fn resolve_term_defid(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
    ) -> Option<DefId> {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.terms.get(name) {
                    return Some(*sym);
                }

                if let Some(sym) = cx.local_api.terms.get(name) {
                    return Some(*sym);
                }

                let mut candidates = Vec::new();

                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.terms.get(name)
                        && !candidates.contains(sym)
                    {
                        candidates.push(*sym);
                    }
                }

                match candidates.as_slice() {
                    [sym] => Some(*sym),
                    [] => {
                        self.report_unknown_name(
                            span,
                            ResolveNameKind::Term,
                            self.symbol_text(*name).to_string(),
                        );
                        None
                    }
                    _ => {
                        self.report_ambiguous_import(
                            span,
                            ResolveNameKind::Term,
                            *name,
                            candidates,
                        );
                        None
                    }
                }
            }

            [_first, rest @ ..] => {
                let local_name = *rest
                    .last()
                    .expect("ICE: qualified term name must have a last segment");

                if cx.module.0.as_slice() == r.prefix() {
                    if let Some(sym) = cx.local_api.terms.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(span, ResolveNameKind::Term, self.unresolved_name(r));
                    return None;
                }

                if let Some(imp) = self.imported_by_alias_or_module(r, cx) {
                    if let Some(sym) = imp.api.terms.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(span, ResolveNameKind::Term, self.unresolved_name(r));
                    return None;
                }

                self.report_unknown_module(span, ModuleName(r.prefix().to_vec()));
                None
            }

            [] => panic!("ICE: empty term name ref"),
        }
    }

    fn resolve_tycon_ref(&mut self, r: &NameRef, span: FileSpan, cx: &ModuleContext) -> String {
        match self.resolve_tycon_defid(r, span, cx) {
            Some(def_id) => {
                self.record_resolution(span, def_id);
                self.legacy_name(def_id)
            }
            None => self.unresolved_name(r),
        }
    }

    fn resolve_tycon_defid(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
    ) -> Option<DefId> {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.types.get(name) {
                    return Some(*sym);
                }

                if let Some(sym) = cx.local_api.types.get(name) {
                    return Some(*sym);
                }

                let mut candidates = Vec::new();

                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.types.get(name)
                        && !candidates.contains(sym)
                    {
                        candidates.push(*sym);
                    }
                }

                match candidates.as_slice() {
                    [sym] => Some(*sym),
                    [] => {
                        self.report_unknown_name(
                            span,
                            ResolveNameKind::TypeConstructor,
                            self.symbol_text(*name).to_string(),
                        );
                        None
                    }
                    _ => {
                        self.report_ambiguous_import(
                            span,
                            ResolveNameKind::TypeConstructor,
                            *name,
                            candidates,
                        );
                        None
                    }
                }
            }

            [_first, rest @ ..] => {
                let local_name = *rest
                    .last()
                    .expect("ICE: qualified type constructor name must have a last segment");

                if cx.module.0.as_slice() == r.prefix() {
                    if let Some(sym) = cx.local_api.types.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(
                        span,
                        ResolveNameKind::TypeConstructor,
                        self.unresolved_name(r),
                    );
                    return None;
                }

                if let Some(imp) = self.imported_by_alias_or_module(r, cx) {
                    if let Some(sym) = imp.api.types.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(
                        span,
                        ResolveNameKind::TypeConstructor,
                        self.unresolved_name(r),
                    );
                    return None;
                }

                self.report_unknown_module(span, ModuleName(r.prefix().to_vec()));
                None
            }

            [] => panic!("ICE: empty type constructor name ref"),
        }
    }

    fn resolve_ctor_ref(&mut self, r: &NameRef, span: FileSpan, cx: &ModuleContext) -> String {
        match self.resolve_ctor_defid(r, span, cx) {
            Some(def_id) => {
                self.record_resolution(span, def_id);
                self.legacy_name(def_id)
            }
            None => self.unresolved_name(r),
        }
    }

    fn resolve_ctor_defid(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
    ) -> Option<DefId> {
        match r.segments.as_slice() {
            [name] => {
                if let Some(sym) = cx.builtins.constructors.get(name) {
                    return Some(*sym);
                }

                if let Some(sym) = cx.local_api.constructors.get(name) {
                    return Some(*sym);
                }

                let mut candidates = Vec::new();

                for imp in &cx.imported_apis {
                    if let Some(sym) = imp.api.constructors.get(name)
                        && !candidates.contains(sym)
                    {
                        candidates.push(*sym);
                    }
                }

                match candidates.as_slice() {
                    [sym] => Some(*sym),
                    [] => {
                        self.report_unknown_name(
                            span,
                            ResolveNameKind::Constructor,
                            self.symbol_text(*name).to_string(),
                        );
                        None
                    }
                    _ => {
                        self.report_ambiguous_import(
                            span,
                            ResolveNameKind::Constructor,
                            *name,
                            candidates,
                        );
                        None
                    }
                }
            }

            [_first, rest @ ..] => {
                let local_name = *rest
                    .last()
                    .expect("ICE: qualified constructor name must have a last segment");

                if cx.module.0.as_slice() == r.prefix() {
                    if let Some(sym) = cx.local_api.constructors.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(
                        span,
                        ResolveNameKind::Constructor,
                        self.unresolved_name(r),
                    );
                    return None;
                }

                if let Some(imp) = self.imported_by_alias_or_module(r, cx) {
                    if let Some(sym) = imp.api.constructors.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(
                        span,
                        ResolveNameKind::Constructor,
                        self.unresolved_name(r),
                    );
                    return None;
                }

                self.report_unknown_module(span, ModuleName(r.prefix().to_vec()));
                None
            }

            [] => panic!("ICE: empty constructor name ref"),
        }
    }

    fn resolve_class_ref(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> String {
        match self.resolve_class_defid(r, span, cx, local) {
            Some(def_id) => {
                self.record_resolution(span, def_id);
                self.legacy_name(def_id)
            }
            None => self.unresolved_name(r),
        }
    }

    fn resolve_class_defid(
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Option<DefId> {
        match r.segments.as_slice() {
            [name] => self.resolve_unqualified_class_defid(name, span, cx, local),

            [_first, rest @ ..] => {
                let local_name = *rest
                    .last()
                    .expect("ICE: qualified class name must have a last segment");

                if cx.module.0.as_slice() == r.prefix() {
                    if let Some(sym) = cx.local_api.classes.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(span, ResolveNameKind::Class, self.unresolved_name(r));
                    return None;
                }

                if let Some(imp) = self.imported_by_alias_or_module(r, cx) {
                    if let Some(sym) = imp.api.classes.get(&local_name) {
                        return Some(*sym);
                    }

                    self.report_unknown_name(span, ResolveNameKind::Class, self.unresolved_name(r));
                    return None;
                }

                self.report_unknown_module(span, ModuleName(r.prefix().to_vec()));
                None
            }

            [] => panic!("ICE: empty class name ref"),
        }
    }

    fn resolve_unqualified_class_defid(
        &mut self,
        name: &Symbol,
        span: FileSpan,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Option<DefId> {
        if let Some(def_id) = local.classes.get(name) {
            return Some(*def_id);
        }

        if let Some(sym) = cx.local_api.classes.get(name) {
            return Some(*sym);
        }

        let mut candidates = Vec::new();

        for imp in &cx.imported_apis {
            if let Some(sym) = imp.api.classes.get(name)
                && !candidates.contains(sym)
            {
                candidates.push(*sym);
            }
        }

        match candidates.as_slice() {
            [sym] => Some(*sym),
            [] => {
                self.report_unknown_name(
                    span,
                    ResolveNameKind::Class,
                    self.symbol_text(*name).to_string(),
                );
                None
            }
            _ => {
                self.report_ambiguous_import(span, ResolveNameKind::Class, *name, candidates);
                None
            }
        }
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
            let def_id = local
                .classes
                .get(&name)
                .or_else(|| cx.local_api.classes.get(&name))
                .or_else(|| {
                    cx.imported_apis
                        .iter()
                        .find_map(|imp| imp.api.classes.get(&name))
                });

            def_id
                .map(|def_id| self.legacy_name(*def_id))
                .unwrap_or(pred.class_name)
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
            .expect("ICE: instance kind checking lost instance head predicate");

        debug_assert_eq!(
            predicates.len(),
            context_len,
            "instance kind checking changed predicate count unexpectedly"
        );

        Ok((predicates, checked_head))
    }

    fn resolve_surface_predicate(
        &mut self,
        pred: &SpannedWrapper<SurfacePredicate>,
        cx: &ModuleContext,
        local: &LocalResolverEnv,
    ) -> Predicate {
        if pred.0.args.len() != 1 {
            self.errors.push(ResolveError::ClassArity {
                span: self.file_span(cx, pred.1),
                class_name: pred.0.class_name.0.display(self.session),
                actual: pred.0.args.len(),
            });
        }

        let resolved = Predicate {
            class_name: self.resolve_class_ref(
                &pred.0.class_name.0,
                self.file_span(cx, pred.0.class_name.1),
                cx,
                local,
            ),
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
        &mut self,
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
        &mut self,
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

            SurfaceTy::Con(con) => {
                TypeExpr::TyCon(self.resolve_tycon_ref(&con.0, self.file_span(cx, con.1), cx))
            }

            SurfaceTy::App(lhs, rhs) => TypeExpr::make_app(
                self.resolve_surface_type_in_module(lhs, cx, local),
                self.resolve_surface_type_in_module(rhs, cx, local),
            ),
        }
    }

    fn resolve_surface_pattern_in_module(
        &mut self,
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
                self.resolve_ctor_ref(&con_name.0, self.file_span(cx, con_name.1), cx),
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
        &mut self,
        r: &NameRef,
        span: FileSpan,
        cx: &ModuleContext,
        bound_terms: &HashSet<Symbol>,
    ) -> String {
        match r.segments.as_slice() {
            [name] if bound_terms.contains(name) => self.symbol_text(*name).to_string(),
            _ => self.resolve_term_ref(r, span, cx),
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
            SurfaceExpr::Var(var_name) => TermExpr::Var(self.resolve_term_ref_with_binders(
                &var_name.0,
                self.file_span(cx, var_name.1),
                cx,
                bound_terms,
            )),
            SurfaceExpr::Con(con_name) => TermExpr::Con(self.resolve_ctor_ref(
                &con_name.0,
                self.file_span(cx, con_name.1),
                cx,
            )),

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
        name: SpannedWrapper<Symbol>,
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
            let Some((arg_ty, ret_ty)) = self.peel_arrow(remaining) else {
                self.errors.push(ResolveError::SignatureArity {
                    span: self.file_span(cx, name.1),
                    name: name.0,
                    params: params.len(),
                });
                break;
            };

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
    ) -> Result<ResolvedModuleInfo<Span>, ResolverError> {
        let local = LocalResolverEnv::from_parsed_module(cx.module_id, parsed);

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
        if let Some(errors) = self.take_errors() {
            return Err(ResolverError::Resolve(errors));
        }

        let (data_defs, local_tycon_kinds, constructor_types, all_tycon_kinds) =
            self.resolve_data_defs(module_name, raw_data_defs, imported)?;

        let class_defs =
            self.resolve_class_defs(module_name, parsed, cx, imported, &local, &all_tycon_kinds)?;
        if let Some(errors) = self.take_errors() {
            return Err(ResolverError::Resolve(errors));
        }

        let class_env = Self::class_param_kind_env(imported, &class_defs);

        let instance_defs = self.resolve_instance_defs(
            module_name,
            parsed,
            cx,
            &local,
            &all_tycon_kinds,
            &class_env,
        )?;
        if let Some(errors) = self.take_errors() {
            return Err(ResolverError::Resolve(errors));
        }

        let func_schemes = self.resolve_signatures(
            module_name,
            parsed,
            cx,
            imported,
            &local,
            &local_tycon_kinds,
            &class_defs,
        )?;
        if let Some(errors) = self.take_errors() {
            return Err(ResolverError::Resolve(errors));
        }

        let func_defs = self.resolve_function_defs(module_name, parsed, cx, &local, &func_schemes);
        if let Some(errors) = self.take_errors() {
            return Err(ResolverError::Resolve(errors));
        }

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

        Ok(ResolvedModuleInfo {
            module: ResolvedModule {
                data_defs,
                class_defs,
                instance_defs,
                tycon_kinds: local_tycon_kinds,
                constructor_types,
                func_schemes,
                func_defs,
            },
            resolutions: self.resolutions,
        })
    }

    fn collect_raw_data_defs(
        &mut self,
        _module_name: &ModuleName,
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
                        name: self.legacy_name(self.local_def_id(cx, *name)),
                        ty_params: ty_params
                            .iter()
                            .map(|p| self.symbol_text(p.0).to_string())
                            .collect(),
                        constructors: constructors
                            .iter()
                            .map(|constructor| RawConstructorDef {
                                name: self.legacy_name(self.local_def_id(cx, constructor.0.name.0)),
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
        &mut self,
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
                    if ty_params.len() != 1 {
                        self.errors.push(ResolveError::ClassArity {
                            span: self.file_span(cx, name.1),
                            class_name: self.symbol_text(name.0).to_string(),
                            actual: ty_params.len(),
                        });
                    }

                    let class_id = self.local_def_id(cx, name.0);
                    let class_name = self.legacy_name(class_id);

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
                                name: self.legacy_name(self.local_def_id(cx, method.name.0)),
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

        if !self.errors.is_empty() {
            return Ok(Vec::new());
        }

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
                    self.file_span(cx, class_name.1),
                    cx,
                    local,
                ),
                args: args
                    .iter()
                    .map(|arg| self.resolve_surface_type_in_module(arg, cx, local))
                    .collect(),
            };

            let raw_head = self.canonicalize_predicate(raw_head, cx, local);

            if args.len() != 1 {
                self.errors.push(ResolveError::ClassArity {
                    span: self.file_span(cx, class_name.1),
                    class_name: self.symbol_text(class_name.0).to_string(),
                    actual: args.len(),
                });
            }

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

            if !self.errors.is_empty() {
                continue;
            }

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
        &mut self,
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
            let fn_name = self.legacy_name(self.local_def_id(cx, *name));
            let raw_scheme = self.resolve_surface_scheme(ty, cx, local);

            trace_predicates(
                self.session,
                module_name,
                &format!("signature {} predicates", fn_name),
                &raw_scheme.predicates,
            );

            if !self.errors.is_empty() {
                continue;
            }

            let inf =
                SignatureKindInferencer::with_envs(sig_tycon_env.clone(), sig_class_env.clone());
            let info = inf.infer_scheme(&raw_scheme)?;

            func_schemes.insert(fn_name, info.scheme.clone());
        }

        Ok(func_schemes)
    }

    fn resolve_function_defs(
        &mut self,
        _module_name: &ModuleName,
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
                    let fn_name = self.legacy_name(self.local_def_id(cx, *name));
                    let sig = func_schemes.get(&fn_name).cloned();

                    let wrapped = match &sig {
                        Some(sig) => self.wrap_function_body_with_signature_in_module(
                            sig,
                            body,
                            params,
                            *spanned_name,
                            cx,
                            local,
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
