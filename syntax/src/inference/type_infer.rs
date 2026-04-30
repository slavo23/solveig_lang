use std::collections::HashMap;

use crate::inference::holes::HoleInfo;
use crate::inference::inference_ctx::{Context, ContextEntry};
use base::utils::diagnostics::{FileSpan, TypeError};
use base::utils::span::Span;
use core::ast::{
    BinOp, CaseArm, CaseArmNode, ClassDefinition, InstanceDefinition, InstanceMethodDef, Kind,
    Literal, Pattern, PatternNode, Predicate, Pretty, ResolvedModule, Scheme, TermDefinition,
    TermExpr, TermExprNode, TypeExpr, UnOp,
};

#[derive(Clone, Debug)]
pub struct TcMetadata {
    pub span: Span,
    pub inferred_type: TypeExpr,
}

type TcResult<T> = Result<T, TypeError>;

#[derive(Clone, Debug)]
struct Wanted {
    pred: Predicate,
    span: Span,
    givens: Vec<Predicate>,
}

impl TcMetadata {
    pub fn new(span: Span, inferred_type: TypeExpr) -> Self {
        Self {
            span,
            inferred_type,
        }
    }
}

fn elab_node(
    node: TermExpr<TcMetadata, Span>,
    span: Span,
    ty: TypeExpr,
) -> TermExprNode<TcMetadata, Span> {
    TermExprNode::new(node, TcMetadata::new(span, ty))
}
pub struct TypeChecker {
    file_id: String,
    counter: usize,
    constructor_types: HashMap<String, TypeExpr>,
    func_schemes: HashMap<String, Scheme>,
    class_defs: Vec<ClassDefinition>,
    instance_defs: Vec<InstanceDefinition<Span>>,
    tycon_kinds: HashMap<String, Kind>,
    givens: Vec<Predicate>,
    wanteds: Vec<Wanted>,
    evar_solutions: HashMap<String, TypeExpr>,
    holes: Vec<HoleInfo>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            file_id: "todo_file.sol".into(),
            counter: 0,
            constructor_types: HashMap::new(),
            func_schemes: HashMap::new(),
            class_defs: Vec::new(),
            instance_defs: Vec::new(),
            tycon_kinds: HashMap::new(),
            givens: Vec::new(),
            wanteds: Vec::new(),
            evar_solutions: HashMap::new(),
            holes: Vec::new(),
        }
    }

    pub fn with_module_context(module: &ResolvedModule<Span>, file_id: String) -> Self {
        Self {
            file_id,
            counter: 0,
            constructor_types: module.constructor_types.clone(),
            func_schemes: module.func_schemes.clone(),
            class_defs: module.class_defs.clone(),
            instance_defs: module.instance_defs.clone(),
            tycon_kinds: module.tycon_kinds.clone(),
            givens: Vec::new(),
            wanteds: Vec::new(),
            evar_solutions: HashMap::new(),
            holes: Vec::new(),
        }
    }

    fn get_file_span(&self, span: Span) -> FileSpan {
        FileSpan {
            file_id: self.file_id.clone(),
            span,
        }
    }

    fn fresh_id(&mut self) -> usize {
        self.counter += 1;
        self.counter
    }

    fn fresh_evar_name(&mut self) -> String {
        format!("a^{}", self.fresh_id())
    }

    fn fresh_universal_name(&mut self, base: &str) -> String {
        format!("{}{}", base, self.fresh_id())
    }

    fn scheme_to_type(scheme: &Scheme) -> TypeExpr {
        let mut ty = scheme.ty.clone();

        if !scheme.predicates.is_empty() {
            ty = TypeExpr::make_qualified(scheme.predicates.clone(), ty);
        }

        for (name, kind) in scheme.quantifiers.iter().rev() {
            ty = TypeExpr::make_forall(name.clone(), kind.clone(), ty);
        }

        ty
    }

    fn type_to_scheme(ty: &TypeExpr) -> Scheme {
        let (quantifiers, predicates, body) = ty.peel_scheme_like();
        Scheme {
            quantifiers,
            predicates,
            ty: body,
        }
    }

    fn zonk_scheme(&self, scheme: &Scheme) -> Scheme {
        Scheme {
            quantifiers: scheme.quantifiers.clone(),
            predicates: scheme
                .predicates
                .iter()
                .map(|pred| core::ast::Predicate {
                    class_name: pred.class_name.clone(),
                    args: pred.args.iter().map(|arg| self.zonk_type(arg)).collect(),
                })
                .collect(),
            ty: self.zonk_type(&scheme.ty),
        }
    }

    fn zonk_predicate(&self, pred: &Predicate) -> Predicate {
        Predicate {
            class_name: pred.class_name.clone(),
            args: pred.args.iter().map(|arg| self.zonk_type(arg)).collect(),
        }
    }

    fn instantiate_predicate_with_subst(
        pred: &Predicate,
        subst: &HashMap<String, TypeExpr>,
    ) -> Predicate {
        Predicate {
            class_name: pred.class_name.clone(),
            args: pred
                .args
                .iter()
                .map(|arg| {
                    subst
                        .iter()
                        .fold(arg.clone(), |acc, (name, ty)| acc.substitute(name, ty))
                })
                .collect(),
        }
    }

    fn instantiate_scheme(
        &mut self,
        ctx: Context,
        scheme: &Scheme,
    ) -> (Context, Vec<Predicate>, TypeExpr) {
        let mut ctx = ctx;
        let mut subst = HashMap::<String, TypeExpr>::new();

        for (name, kind) in &scheme.quantifiers {
            let fresh = self.fresh_evar_name();
            ctx = ctx.add_entry(ContextEntry::ExVar(fresh.clone(), kind.clone()));
            subst.insert(name.clone(), TypeExpr::TyExVar(fresh));
        }

        let instantiated_preds = scheme
            .predicates
            .iter()
            .map(|pred| Self::instantiate_predicate_with_subst(pred, &subst))
            .collect::<Vec<_>>();

        let instantiated_ty = subst.iter().fold(scheme.ty.clone(), |acc, (name, ty)| {
            acc.substitute(name, ty)
        });

        (ctx, instantiated_preds, instantiated_ty)
    }

    fn lookup_class_method_scheme(&self, name: &str) -> Option<&Scheme> {
        self.class_defs.iter().find_map(|class_def| {
            class_def.methods.iter().find_map(|method| {
                let short_name = method.name.rsplit('.').next().unwrap_or(&method.name);
                if short_name == name {
                    Some(&method.scheme)
                } else {
                    None
                }
            })
        })
    }

    fn add_wanteds(&mut self, preds: Vec<Predicate>, span: Span) {
        let givens = self
            .givens
            .iter()
            .map(|pred| self.zonk_predicate(pred))
            .collect::<Vec<_>>();

        let zonked = preds
            .into_iter()
            .map(|pred| Wanted {
                pred: self.zonk_predicate(&pred),
                span: span.clone(),
                givens: givens.clone(),
            })
            .collect::<Vec<_>>();

        self.wanteds.extend(zonked);
    }

    fn with_givens<R>(&mut self, preds: Vec<Predicate>, f: impl FnOnce(&mut Self) -> R) -> R {
        let old_len = self.givens.len();
        self.givens.extend(preds);
        let result = f(self);
        self.givens.truncate(old_len);
        result
    }

    fn match_instance_type(
        pattern: &TypeExpr,
        target: &TypeExpr,
        subst: &mut HashMap<String, TypeExpr>,
    ) -> bool {
        match pattern {
            TypeExpr::TyVar(name) => match subst.get(name) {
                Some(existing) => existing == target,
                None => {
                    subst.insert(name.clone(), target.clone());
                    true
                }
            },

            TypeExpr::TyCon(a) => matches!(target, TypeExpr::TyCon(b) if a == b),
            TypeExpr::TyUnit => matches!(target, TypeExpr::TyUnit),

            TypeExpr::TyApp(f1, a1) => match target {
                TypeExpr::TyApp(f2, a2) => {
                    Self::match_instance_type(f1, f2, subst)
                        && Self::match_instance_type(a1, a2, subst)
                }
                _ => false,
            },

            TypeExpr::TyArr(a1, b1) => match target {
                TypeExpr::TyArr(a2, b2) => {
                    Self::match_instance_type(a1, a2, subst)
                        && Self::match_instance_type(b1, b2, subst)
                }
                _ => false,
            },

            TypeExpr::TyForAll(_, _, body) => Self::match_instance_type(body, target, subst),
            TypeExpr::Qualified(_, body) => Self::match_instance_type(body, target, subst),

            TypeExpr::TyExVar(name) => matches!(target, TypeExpr::TyExVar(other) if name == other),
        }
    }

    fn discharge_predicate_with_givens(&self, pred: &Predicate, givens: &[Predicate]) -> bool {
        let pred = self.zonk_predicate(pred);

        if givens
            .iter()
            .any(|given| self.zonk_predicate(given) == pred)
        {
            return true;
        }

        for inst in &self.instance_defs {
            if inst.head.class_name != pred.class_name || inst.head.args.len() != pred.args.len() {
                continue;
            }

            let mut subst = HashMap::<String, TypeExpr>::new();

            let head_matches =
                inst.head
                    .args
                    .iter()
                    .zip(pred.args.iter())
                    .all(|(head_arg, wanted_arg)| {
                        let wanted_arg = self.zonk_type(wanted_arg);
                        Self::match_instance_type(head_arg, &wanted_arg, &mut subst)
                    });

            if !head_matches {
                continue;
            }

            let all_context_ok = inst.context.iter().all(|ctx_pred| {
                let instantiated = Self::instantiate_predicate_with_subst(ctx_pred, &subst);
                self.discharge_predicate_with_givens(&instantiated, givens)
            });

            if all_context_ok {
                return true;
            }
        }

        false
    }

    fn discharge_predicate(&self, pred: &Predicate) -> bool {
        self.discharge_predicate_with_givens(pred, &self.givens)
    }

    fn unresolved_wanteds(&self) -> Vec<Wanted> {
        for wanted in &self.wanteds {
            eprintln!(
                "wanted full = {:?}, givens full = {:?}",
                self.zonk_predicate(&wanted.pred),
                wanted
                    .givens
                    .iter()
                    .map(|g| self.zonk_predicate(g))
                    .collect::<Vec<_>>()
            );
        }

        for inst in &self.instance_defs {
            eprintln!("instance head full = {:?}", self.zonk_predicate(&inst.head));
        }
        self.wanteds
            .iter()
            .cloned()
            .filter(|wanted| !self.discharge_predicate_with_givens(&wanted.pred, &wanted.givens))
            .collect()
    }

    fn wanteds_error(&self, wanteds: Vec<Wanted>) -> TypeError {
        let span = wanteds
            .first()
            .map(|w| self.get_file_span(w.span.clone()))
            .unwrap_or_else(|| self.get_file_span(0..0));

        TypeError::UnsatisfiedConstraints {
            span,
            predicates: wanteds
                .into_iter()
                .map(|w| self.zonk_predicate(&w.pred).pretty())
                .collect(),
        }
    }

    fn record_solution(&mut self, solved_entry: &ContextEntry) {
        if let ContextEntry::SolvedEVar {
            lhs: evar_name,
            rhs: solution,
        } = solved_entry
        {
            self.evar_solutions
                .insert(evar_name.clone(), solution.clone());
        }
    }

    fn context_bindings(&self, ctx: &Context) -> Vec<(String, TypeExpr)> {
        ctx.get_entries()
            .iter()
            .filter_map(|entry| {
                if let ContextEntry::TypedVar { var_name, ty } = entry {
                    Some((var_name.clone(), self.zonk_type(&ctx.apply(&ty))))
                } else {
                    None
                }
            })
            .collect()
    }

    fn report_hole(&mut self, ctx: &Context, span: Span, ty: &TypeExpr) {
        self.holes.push(HoleInfo {
            span: self.get_file_span(span),
            expected_type: self.zonk_type(&ctx.apply(ty)),
            context: self.context_bindings(ctx),
        });
    }

    pub fn finalize_holes(&mut self) {
        let mut holes = std::mem::take(&mut self.holes);
        for hole in &mut holes {
            hole.expected_type = self.zonk_type(&hole.expected_type);
            for (_, ty) in &mut hole.context {
                *ty = self.zonk_type(ty);
            }
        }
        self.holes = holes;
    }

    fn bool_type(&self) -> TypeExpr {
        TypeExpr::TyCon("prelude.builtins.Bool".to_string())
    }

    fn collect_exvars_in_type(ty: &TypeExpr, acc: &mut Vec<String>) {
        match ty {
            TypeExpr::TyExVar(name) => {
                if !acc.contains(name) {
                    acc.push(name.clone());
                }
            }
            TypeExpr::TyArr(lhs, rhs) | TypeExpr::TyApp(lhs, rhs) => {
                Self::collect_exvars_in_type(lhs, acc);
                Self::collect_exvars_in_type(rhs, acc);
            }
            TypeExpr::TyForAll(_, _, body) => {
                Self::collect_exvars_in_type(body, acc);
            }
            TypeExpr::Qualified(preds, body) => {
                for pred in preds {
                    for arg in &pred.args {
                        Self::collect_exvars_in_type(arg, acc);
                    }
                }
                Self::collect_exvars_in_type(body, acc);
            }
            TypeExpr::TyUnit | TypeExpr::TyVar(_) | TypeExpr::TyCon(_) => {}
        }
    }

    fn generalize_top_level_type(&mut self, ctx: &Context, ty: &TypeExpr) -> TypeExpr {
        let zonked = self.zonk_type(&ctx.apply(ty));

        let mut evars = Vec::new();
        Self::collect_exvars_in_type(&zonked, &mut evars);

        if evars.is_empty() {
            return zonked;
        }

        let mut result = zonked;
        let mut binders = Vec::new();

        for evar in evars {
            let universal = self.fresh_universal_name("t");
            let kind = ctx.evar_kind(&evar).unwrap_or(Kind::Star);

            result = result.substitute(&evar, &TypeExpr::TyVar(universal.clone()));
            binders.push((universal, kind));
        }

        for (name, kind) in binders.into_iter().rev() {
            result = TypeExpr::make_forall(name, kind, result);
        }

        result
    }

    pub fn zonk_type(&self, ty: &TypeExpr) -> TypeExpr {
        match ty {
            TypeExpr::TyExVar(name) => {
                if let Some(solution) = self.evar_solutions.get(name) {
                    self.zonk_type(solution)
                } else {
                    ty.clone()
                }
            }
            TypeExpr::TyArr(lhs, rhs) => {
                TypeExpr::make_arr(self.zonk_type(lhs), self.zonk_type(rhs))
            }
            TypeExpr::TyApp(lhs, rhs) => {
                TypeExpr::make_app(self.zonk_type(lhs), self.zonk_type(rhs))
            }
            TypeExpr::TyForAll(quantifier, kind, body) => {
                TypeExpr::make_forall(quantifier.clone(), kind.clone(), self.zonk_type(body))
            }
            TypeExpr::Qualified(preds, body) => TypeExpr::make_qualified(
                preds
                    .iter()
                    .map(|pred| core::ast::Predicate {
                        class_name: pred.class_name.clone(),
                        args: pred.args.iter().map(|arg| self.zonk_type(arg)).collect(),
                    })
                    .collect(),
                self.zonk_type(body),
            ),
            _ => ty.clone(),
        }
    }

    pub fn zonk_expr(
        &self,
        expr: &TermExprNode<TcMetadata, Span>,
    ) -> TermExprNode<TcMetadata, Span> {
        let zonked_meta = TcMetadata::new(
            expr.anno.span.clone(),
            self.zonk_type(&expr.anno.inferred_type),
        );
        let zonked_node = match &expr.node {
            TermExpr::Hole => TermExpr::Hole,
            TermExpr::Unit => TermExpr::Unit,
            TermExpr::Var(name) => TermExpr::Var(name.clone()),
            TermExpr::Lit(lit_val) => TermExpr::Lit(lit_val.clone()),
            TermExpr::Con(name) => TermExpr::Con(name.clone()),
            TermExpr::App(func, arg) => TermExpr::App(
                Box::new(self.zonk_expr(func)),
                Box::new(self.zonk_expr(arg)),
            ),
            TermExpr::Abs(param, annotation, body) => TermExpr::Abs(
                param.clone(),
                annotation.as_ref().map(|ty| self.zonk_type(ty)),
                Box::new(self.zonk_expr(body)),
            ),
            TermExpr::TyAbs(param, kind, body) => {
                TermExpr::TyAbs(param.clone(), kind.clone(), Box::new(self.zonk_expr(body)))
            }
            TermExpr::TyApp(inner, ty_arg) => {
                TermExpr::TyApp(Box::new(self.zonk_expr(inner)), self.zonk_type(ty_arg))
            }
            TermExpr::Anno(inner, ty) => {
                TermExpr::Anno(Box::new(self.zonk_expr(inner)), self.zonk_type(ty))
            }
            TermExpr::Case(scrutinee, arms) => TermExpr::Case(
                Box::new(self.zonk_expr(scrutinee)),
                arms.iter()
                    .map(|arm| {
                        CaseArmNode::new(
                            CaseArm {
                                pattern: arm.node.pattern.clone(),
                                expr: self.zonk_expr(&arm.node.expr),
                            },
                            arm.anno.clone(),
                        )
                    })
                    .collect(),
            ),
            TermExpr::IfThenElse(cond, then_branch, else_branch) => TermExpr::IfThenElse(
                Box::new(self.zonk_expr(cond)),
                Box::new(self.zonk_expr(then_branch)),
                Box::new(self.zonk_expr(else_branch)),
            ),
            TermExpr::BinaryOperator(op, lhs, rhs) => TermExpr::BinaryOperator(
                *op,
                Box::new(self.zonk_expr(lhs)),
                Box::new(self.zonk_expr(rhs)),
            ),
            TermExpr::UnaryOperator(op, rhs) => {
                TermExpr::UnaryOperator(*op, Box::new(self.zonk_expr(rhs)))
            }
            TermExpr::Let(binder_name, binder_expr, in_expr) => TermExpr::Let(
                binder_name.clone(),
                Box::new(self.zonk_expr(binder_expr)),
                Box::new(self.zonk_expr(in_expr)),
            ),
        };
        TermExprNode::new(zonked_node, zonked_meta)
    }

    fn lookup_local_term_var(&self, ctx: &Context, name: &str) -> Option<TypeExpr> {
        ctx.lookup_term_var(name)
            .or_else(|| self.constructor_types.get(name).cloned())
    }

    pub fn solve_subtype(
        &mut self,
        ctx: Context,
        left: &TypeExpr,
        right: &TypeExpr,
        origin_span: FileSpan,
    ) -> Result<Context, TypeError> {
        let original_left = ctx.apply(left);
        let original_right = ctx.apply(right);

        match self.solve_subtype_inner(ctx, &original_left, &original_right) {
            Ok(delta) => Ok(delta),
            Err(()) => Err(TypeError::SubtypeMismatch {
                span: origin_span,
                left: left.pretty(),
                right: right.pretty(),
            }),
        }
    }
    pub fn solve_subtype_inner(
        &mut self,
        ctx: Context,
        left: &TypeExpr,
        right: &TypeExpr,
    ) -> Result<Context, ()> {
        let left = &ctx.apply(left);
        let right = &ctx.apply(right);

        if left == right {
            return Ok(ctx);
        }

        match (left, right) {
            (TypeExpr::Qualified(_, body), _) => self.solve_subtype_inner(ctx, body, right),
            (_, TypeExpr::Qualified(_, body)) => self.solve_subtype_inner(ctx, left, body),

            (TypeExpr::TyUnit, TypeExpr::TyUnit) => Ok(ctx),
            (TypeExpr::TyVar(name_a), TypeExpr::TyVar(name_b)) if name_a == name_b => Ok(ctx),
            (TypeExpr::TyExVar(name_a), TypeExpr::TyExVar(name_b)) if name_a == name_b => Ok(ctx),
            (TypeExpr::TyCon(name_a), TypeExpr::TyCon(name_b)) if name_a == name_b => Ok(ctx),

            (TypeExpr::TyArr(domain_a, codomain_a), TypeExpr::TyArr(domain_b, codomain_b)) => {
                let theta = self.solve_subtype_inner(ctx, domain_b, domain_a)?;
                let codomain_a_applied = theta.apply(codomain_a);
                let codomain_b_applied = theta.apply(codomain_b);
                self.solve_subtype_inner(theta, &codomain_a_applied, &codomain_b_applied)
            }

            (TypeExpr::TyApp(func_a, arg_a), TypeExpr::TyApp(func_b, arg_b)) => {
                let theta = self.solve_subtype_inner(ctx, func_b, func_a)?;
                let arg_a_applied = theta.apply(arg_a);
                let arg_b_applied = theta.apply(arg_b);
                self.solve_subtype_inner(theta, &arg_a_applied, &arg_b_applied)
            }

            (TypeExpr::TyForAll(quantifier, kind, body), _) => {
                let fresh_evar = self.fresh_evar_name();
                let marker = ContextEntry::Marker {
                    var_name: fresh_evar.clone(),
                };
                let extended_ctx = ctx
                    .add_entry(marker.clone())
                    .add_entry(ContextEntry::ExVar(fresh_evar.clone(), kind.clone()));

                let instantiated_body = body.substitute(quantifier, &TypeExpr::TyExVar(fresh_evar));
                let theta = self.solve_subtype_inner(extended_ctx, &instantiated_body, right)?;
                Ok(theta.until(&marker))
            }

            (_, TypeExpr::TyForAll(quantifier, kind, body)) => {
                let universal_entry = ContextEntry::UniVar(quantifier.clone(), kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());
                let theta = self.solve_subtype_inner(extended_ctx, left, body)?;
                Ok(theta.until(&universal_entry))
            }

            (TypeExpr::TyExVar(evar_name), _)
                if ctx.has_unsolved_evar(evar_name) && !right.occurs_check(evar_name.clone()) =>
            {
                self.instantiate_left(ctx, evar_name, right)
            }

            (_, TypeExpr::TyExVar(evar_name))
                if ctx.has_unsolved_evar(evar_name) && !left.occurs_check(evar_name.clone()) =>
            {
                self.instantiate_right(ctx, left, evar_name)
            }

            _ => Err(()),
        }
    }

    fn instantiate_left(
        &mut self,
        ctx: Context,
        evar_name: &str,
        target: &TypeExpr,
    ) -> Result<Context, ()> {
        let evar_entry = ContextEntry::ExVar(
            evar_name.to_string(),
            ctx.evar_kind(evar_name)
                .unwrap_or_else(|| panic!("missing evar kind for {}", evar_name)),
        );
        let (mut left_entries, _, right_entries) = ctx.split_at(&evar_entry);

        let left_ctx = Context::from_entries(left_entries.clone());
        if left_ctx.type_well_formed(target) && target.is_mono() {
            let solution = ContextEntry::SolvedEVar {
                lhs: evar_name.to_string(),
                rhs: target.clone(),
            };
            self.record_solution(&solution);
            left_entries.push(solution);
            left_entries.extend(right_entries);
            return Ok(Context::from_entries(left_entries));
        }

        match target {
            TypeExpr::TyExVar(other_evar) if ctx.evar_before(evar_name, other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: other_evar.clone(),
                    rhs: TypeExpr::TyExVar(evar_name.to_string()),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(
                    &ContextEntry::ExVar(
                        other_evar.clone(),
                        ctx.evar_kind(other_evar)
                            .unwrap_or_else(|| panic!("missing evar kind for {}", other_evar)),
                    ),
                    &solution,
                ))
            }
            TypeExpr::TyExVar(other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: evar_name.to_string(),
                    rhs: TypeExpr::TyExVar(other_evar.clone()),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(&evar_entry, &solution))
            }
            TypeExpr::TyArr(domain, codomain) => {
                let domain_evar = self.fresh_evar_name();
                let codomain_evar = self.fresh_evar_name();
                let arrow_solution = ContextEntry::SolvedEVar {
                    lhs: evar_name.to_string(),
                    rhs: TypeExpr::make_arr(
                        TypeExpr::TyExVar(domain_evar.clone()),
                        TypeExpr::TyExVar(codomain_evar.clone()),
                    ),
                };
                self.record_solution(&arrow_solution);

                left_entries.push(ContextEntry::ExVar(codomain_evar.clone(), Kind::Star));
                left_entries.push(ContextEntry::ExVar(domain_evar.clone(), Kind::Star));
                left_entries.push(arrow_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);

                let theta = self.instantiate_right(new_ctx, domain, &domain_evar)?;
                let codomain_applied = theta.apply(codomain);
                self.instantiate_left(theta, &codomain_evar, &codomain_applied)
            }
            TypeExpr::TyApp(func_part, arg_part) => {
                let func_evar = self.fresh_evar_name();
                let arg_evar = self.fresh_evar_name();
                let app_solution = ContextEntry::SolvedEVar {
                    lhs: evar_name.to_string(),
                    rhs: TypeExpr::make_app(
                        TypeExpr::TyExVar(func_evar.clone()),
                        TypeExpr::TyExVar(arg_evar.clone()),
                    ),
                };
                self.record_solution(&app_solution);

                left_entries.push(ContextEntry::ExVar(arg_evar.clone(), Kind::Star));
                left_entries.push(ContextEntry::ExVar(func_evar.clone(), Kind::Star));
                left_entries.push(app_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);

                let theta = self.instantiate_left(new_ctx, &func_evar, func_part)?;
                let arg_applied = theta.apply(arg_part);
                self.instantiate_left(theta, &arg_evar, &arg_applied)
            }
            TypeExpr::TyForAll(quantifier, kind, body) => {
                let universal_entry = ContextEntry::UniVar(quantifier.clone(), kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());
                let delta = self.instantiate_left(extended_ctx, evar_name, body)?;
                Ok(delta.until(&universal_entry))
            }
            TypeExpr::Qualified(_, body) => self.instantiate_left(ctx, evar_name, body),

            _ => todo!("instantiate left failed"),
        }
    }

    fn instantiate_right(
        &mut self,
        ctx: Context,
        source: &TypeExpr,
        evar_name: &str,
    ) -> Result<Context, ()> {
        let evar_entry = ContextEntry::ExVar(
            evar_name.to_string(),
            ctx.evar_kind(evar_name)
                .unwrap_or_else(|| panic!("missing evar kind for {}", evar_name)),
        );
        let (mut left_entries, _, right_entries) = ctx.split_at(&evar_entry);
        let left_ctx = Context::from_entries(left_entries.clone());
        if left_ctx.type_well_formed(source) && source.is_mono() {
            let solution = ContextEntry::SolvedEVar {
                lhs: evar_name.to_string(),
                rhs: source.clone(),
            };
            self.record_solution(&solution);
            left_entries.push(solution);
            left_entries.extend(right_entries);
            return Ok(Context::from_entries(left_entries));
        }

        match source {
            TypeExpr::TyExVar(other_evar) if ctx.evar_before(evar_name, other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: other_evar.clone(),
                    rhs: TypeExpr::TyExVar(evar_name.to_string()),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(
                    &ContextEntry::ExVar(
                        other_evar.clone(),
                        ctx.evar_kind(other_evar)
                            .unwrap_or_else(|| panic!("missing evar kind for {}", other_evar)),
                    ),
                    &solution,
                ))
            }

            TypeExpr::TyExVar(other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: evar_name.to_string(),
                    rhs: TypeExpr::TyExVar(other_evar.clone()),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(&evar_entry, &solution))
            }
            TypeExpr::TyArr(domain, codomain) => {
                let domain_evar = self.fresh_evar_name();
                let codomain_evar = self.fresh_evar_name();
                let arrow_solution = ContextEntry::SolvedEVar {
                    lhs: evar_name.to_string(),
                    rhs: TypeExpr::make_arr(
                        TypeExpr::TyExVar(domain_evar.clone()),
                        TypeExpr::TyExVar(codomain_evar.clone()),
                    ),
                };
                self.record_solution(&arrow_solution);

                left_entries.push(ContextEntry::ExVar(codomain_evar.clone(), Kind::Star));
                left_entries.push(ContextEntry::ExVar(domain_evar.clone(), Kind::Star));
                left_entries.push(arrow_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);
                let theta = self.instantiate_left(new_ctx, &domain_evar, domain)?;
                let codomain_applied = theta.apply(codomain);
                self.instantiate_right(theta, &codomain_applied, &codomain_evar)
            }
            TypeExpr::TyApp(func_part, arg_part) => {
                let func_evar = self.fresh_evar_name();
                let arg_evar = self.fresh_evar_name();
                let app_solution = ContextEntry::SolvedEVar {
                    lhs: evar_name.to_string(),
                    rhs: TypeExpr::make_app(
                        TypeExpr::TyExVar(func_evar.clone()),
                        TypeExpr::TyExVar(arg_evar.clone()),
                    ),
                };
                self.record_solution(&app_solution);

                left_entries.push(ContextEntry::ExVar(arg_evar.clone(), Kind::Star));
                left_entries.push(ContextEntry::ExVar(func_evar.clone(), Kind::Star));
                left_entries.push(app_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);

                let theta = self.instantiate_right(new_ctx, func_part, &func_evar)?;
                let arg_applied = theta.apply(arg_part);
                self.instantiate_right(theta, &arg_applied, &arg_evar)
            }
            TypeExpr::TyForAll(quantifier, kind, body) => {
                let fresh_evar = self.fresh_evar_name();
                let marker = ContextEntry::Marker {
                    var_name: fresh_evar.clone(),
                };
                let extended_ctx = ctx
                    .add_entry(marker.clone())
                    .add_entry(ContextEntry::ExVar(fresh_evar.clone(), kind.clone()));

                let instantiated = body.substitute(quantifier, &TypeExpr::TyExVar(fresh_evar));
                let delta = self.instantiate_right(extended_ctx, &instantiated, evar_name)?;
                Ok(delta.until(&marker))
            }
            TypeExpr::Qualified(_, body) => self.instantiate_right(ctx, body, evar_name),

            _ => todo!("instantiate right failed"),
        }
    }

    fn force_arrow(
        &mut self,
        ctx: Context,
        ty: &TypeExpr,
    ) -> TcResult<(Context, TypeExpr, TypeExpr)> {
        let ty = ctx.apply(ty);

        match &ty {
            TypeExpr::TyArr(domain, codomain) => {
                Ok((ctx, (**domain).clone(), (**codomain).clone()))
            }

            TypeExpr::TyForAll(quantifier, kind, body) => {
                let fresh_evar = self.fresh_evar_name();
                let new_ctx = ctx.add_entry(ContextEntry::ExVar(fresh_evar.clone(), kind.clone()));
                let instantiated = body.substitute(quantifier, &TypeExpr::TyExVar(fresh_evar));
                self.force_arrow(new_ctx, &instantiated)
            }
            TypeExpr::Qualified(_, body) => self.force_arrow(ctx, body),

            TypeExpr::TyExVar(evar_name) if ctx.has_unsolved_evar(evar_name) => {
                let evar_entry = ContextEntry::ExVar(
                    evar_name.clone(),
                    ctx.evar_kind(evar_name)
                        .unwrap_or_else(|| panic!("missing evar kind for {}", evar_name)),
                );
                let (mut left_entries, _, right_entries) = ctx.split_at(&evar_entry);

                let domain_evar = self.fresh_evar_name();
                let codomain_evar = self.fresh_evar_name();
                let arrow_solution = ContextEntry::SolvedEVar {
                    lhs: evar_name.clone(),
                    rhs: TypeExpr::make_arr(
                        TypeExpr::TyExVar(domain_evar.clone()),
                        TypeExpr::TyExVar(codomain_evar.clone()),
                    ),
                };
                self.record_solution(&arrow_solution);

                left_entries.push(ContextEntry::ExVar(codomain_evar.clone(), Kind::Star));
                left_entries.push(ContextEntry::ExVar(domain_evar.clone(), Kind::Star));
                left_entries.push(arrow_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);

                Ok((
                    new_ctx,
                    TypeExpr::TyExVar(domain_evar),
                    TypeExpr::TyExVar(codomain_evar),
                ))
            }

            _ => todo!("force arrow failed"),
        }
    }

    fn check_optional_param_annotation(
        &mut self,
        ctx: Context,
        maybe_annotation: &Option<TypeExpr>,
        expected_domain: &TypeExpr,
        span: Span,
    ) -> TcResult<Context> {
        match maybe_annotation {
            Some(annotation) => {
                let origin = self.get_file_span(span);
                let ctx = self.solve_subtype(
                    ctx.clone(),
                    &ctx.apply(expected_domain),
                    &ctx.apply(annotation),
                    origin.clone(),
                )?;
                self.solve_subtype(
                    ctx.clone(),
                    &ctx.apply(annotation),
                    &ctx.apply(expected_domain),
                    origin,
                )
            }
            None => Ok(ctx),
        }
    }

    fn instantiate_constructor_pattern(
        &mut self,
        ctx: Context,
        constructor_type: &TypeExpr,
        arity: usize,
    ) -> TcResult<(Context, Vec<TypeExpr>, TypeExpr)> {
        let mut theta = ctx;
        let mut current_type = constructor_type.clone();
        let mut field_types = Vec::with_capacity(arity);

        for _ in 0..arity {
            let (new_ctx, domain, codomain) = self.force_arrow(theta, &current_type)?;
            theta = new_ctx;
            field_types.push(domain);
            current_type = codomain;
        }

        Ok((theta, field_types, current_type))
    }

    fn add_pattern_bindings(&self, mut ctx: Context, bindings: &[(String, TypeExpr)]) -> Context {
        for (var_name, var_type) in bindings {
            ctx = ctx.add_entry(ContextEntry::TypedVar {
                var_name: var_name.clone(),
                ty: var_type.clone(),
            });
        }
        ctx
    }

    fn solve_case_inference(
        &mut self,
        ctx: Context,
        scrutinee: &TermExprNode<Span>,
        arms: &[CaseArmNode<Span>],
        span: Span,
    ) -> TcResult<(Context, TermExprNode<TcMetadata, Span>, TypeExpr)> {
        if arms.is_empty() {
            return Err(TypeError::EmptyCaseExpression {
                span: self.get_file_span(span.clone()),
            });
        }

        let (gamma, elaborated_scrutinee, scrutinee_type) = self.solve_inference(ctx, scrutinee)?;
        let scrutinee_type = gamma.apply(&scrutinee_type);

        let result_evar = self.fresh_evar_name();
        let result_type = TypeExpr::TyExVar(result_evar.clone());
        let mut theta = gamma.add_entry(ContextEntry::ExVar(result_evar.clone(), Kind::Star));

        let mut elaborated_arms: Vec<CaseArmNode<TcMetadata, Span>> =
            Vec::with_capacity(arms.len());

        for (arm_index, arm) in arms.iter().enumerate() {
            let arm_marker = ContextEntry::Marker {
                var_name: format!("case_arm#{}", arm_index),
            };

            let arm_ctx = theta.clone().add_entry(arm_marker.clone());
            let scrutinee_in_arm = arm_ctx.apply(&scrutinee_type);

            let (delta, bindings) =
                self.check_pattern(arm_ctx, &arm.node.pattern, &scrutinee_in_arm)?;

            let body_ctx = self.add_pattern_bindings(delta, &bindings);
            let arm_result_type = body_ctx.apply(&result_type);

            let (omega, elaborated_body) =
                self.solve_checking(body_ctx, &arm.node.expr, &arm_result_type)?;

            if !omega.has_unsolved_evar(&result_evar) && !omega.has_solved_evar(&result_evar) {
                panic!(
                    "internal compiler error: missing case result existential `{}` after arm {}",
                    result_evar, arm_index
                );
            }

            theta = omega.until(&arm_marker);

            elaborated_arms.push(CaseArmNode::new(
                CaseArm {
                    pattern: arm.node.pattern.clone(),
                    expr: elaborated_body,
                },
                arm.anno.clone(),
            ));
        }

        let final_type = theta.apply(&result_type);
        let elaborated = elab_node(
            TermExpr::Case(Box::new(elaborated_scrutinee), elaborated_arms),
            span,
            final_type.clone(),
        );

        Ok((theta, elaborated, final_type))
    }

    fn check_pattern(
        &mut self,
        ctx: Context,
        pattern: &PatternNode<Span>,
        scrutinee_type: &TypeExpr,
    ) -> TcResult<(Context, Vec<(String, TypeExpr)>)> {
        let scrutinee_type = ctx.apply(scrutinee_type);

        match &pattern.node {
            Pattern::Wild => Ok((ctx, vec![])),

            Pattern::Var(name) => Ok((ctx, vec![(name.clone(), scrutinee_type)])),

            Pattern::Con(constructor_name, sub_patterns) => {
                let constructor_type = self
                    .constructor_types
                    .get(constructor_name)
                    .cloned()
                    .ok_or_else(|| TypeError::UnknownPatternConstructor {
                        span: self.get_file_span(pattern.anno.clone()),
                        name: constructor_name.clone(),
                    })?;

                let (mut theta, field_types, constructor_result_type) = self
                    .instantiate_constructor_pattern(ctx, &constructor_type, sub_patterns.len())?;

                theta = self.solve_subtype(
                    theta.clone(),
                    &theta.apply(&constructor_result_type),
                    &theta.apply(&scrutinee_type),
                    self.get_file_span(pattern.anno.clone()),
                )?;

                let mut bindings = Vec::new();
                for (sub_pattern, field_type) in sub_patterns.iter().zip(field_types.iter()) {
                    let field_type_applied = theta.apply(field_type);
                    let (new_ctx, sub_bindings) =
                        self.check_pattern(theta, sub_pattern, &field_type_applied)?;
                    theta = new_ctx;
                    bindings.extend(sub_bindings);
                }

                Ok((theta, bindings))
            }
        }
    }

    pub fn solve_checking(
        &mut self,
        ctx: Context,
        expr: &TermExprNode<Span>,
        expected_type: &TypeExpr,
    ) -> TcResult<(Context, TermExprNode<TcMetadata, Span>)> {
        let expected_type = &ctx.apply(expected_type);
        let span = expr.anno.clone();

        match (&expr.node, expected_type) {
            (TermExpr::Unit, TypeExpr::TyUnit) => {
                let elaborated = elab_node(TermExpr::Unit, span, TypeExpr::TyUnit);
                Ok((ctx, elaborated))
            }

            (
                TermExpr::TyAbs(param, _kind, body),
                TypeExpr::TyForAll(quantifier, quant_kind, body_type),
            ) => {
                let substituted_body_type =
                    body_type.substitute(quantifier, &TypeExpr::TyVar(param.clone()));
                let universal_entry = ContextEntry::UniVar(param.clone(), quant_kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());

                let (delta, elaborated_body) =
                    self.solve_checking(extended_ctx, body, &substituted_body_type)?;

                let result_type = expected_type.clone();
                let elaborated = elab_node(
                    TermExpr::TyAbs(param.clone(), quant_kind.clone(), Box::new(elaborated_body)),
                    span,
                    result_type,
                );
                Ok((delta.until(&universal_entry), elaborated))
            }

            (TermExpr::Abs(param, maybe_annotation, body), TypeExpr::TyArr(domain, codomain)) => {
                let ctx = self.check_optional_param_annotation(
                    ctx,
                    maybe_annotation,
                    domain,
                    span.clone(),
                )?;

                let typed_var_entry = ContextEntry::TypedVar {
                    var_name: param.clone(),
                    ty: (**domain).clone(),
                };
                let extended_ctx = ctx.add_entry(typed_var_entry.clone());

                let (delta, elaborated_body) = self.solve_checking(extended_ctx, body, codomain)?;

                let result_type = expected_type.clone();
                let elaborated = elab_node(
                    TermExpr::Abs(
                        param.clone(),
                        Some(delta.apply(domain)),
                        Box::new(elaborated_body),
                    ),
                    span,
                    result_type,
                );
                Ok((delta.until(&typed_var_entry), elaborated))
            }
            (TermExpr::Let(name, rhs, body), _) => {
                let (gamma, elaborated_rhs, rhs_type) = self.solve_inference(ctx, rhs)?;
                let rhs_type = gamma.apply(&rhs_type);

                let typed_var_entry = ContextEntry::TypedVar {
                    var_name: name.clone(),
                    ty: rhs_type,
                };
                let extended_ctx = gamma.add_entry(typed_var_entry.clone());

                let (delta, elaborated_body) =
                    self.solve_checking(extended_ctx, body, expected_type)?;
                let result_type = delta.apply(expected_type);

                let elaborated = elab_node(
                    TermExpr::Let(
                        name.clone(),
                        Box::new(elaborated_rhs),
                        Box::new(elaborated_body),
                    ),
                    span,
                    result_type,
                );

                Ok((delta.until(&typed_var_entry), elaborated))
            }
            (TermExpr::IfThenElse(cond, then_branch, else_branch), _) => {
                let bool_type = self.bool_type();
                let (gamma, elaborated_cond) = self.solve_checking(ctx, cond, &bool_type)?;
                let (gamma, elaborated_then) =
                    self.solve_checking(gamma, then_branch, expected_type)?;
                let (gamma, elaborated_else) =
                    self.solve_checking(gamma, else_branch, expected_type)?;

                let result_type = gamma.apply(expected_type);
                let elaborated = elab_node(
                    TermExpr::IfThenElse(
                        Box::new(elaborated_cond),
                        Box::new(elaborated_then),
                        Box::new(elaborated_else),
                    ),
                    span,
                    result_type,
                );
                Ok((gamma, elaborated))
            }

            (TermExpr::Hole, _) => {
                self.report_hole(&ctx, span.clone(), expected_type);
                let elaborated = elab_node(TermExpr::Hole, span, expected_type.clone());
                Ok((ctx, elaborated))
            }

            (_, TypeExpr::TyForAll(quantifier, kind, body_type)) => {
                let universal_entry = ContextEntry::UniVar(quantifier.clone(), kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());

                let (delta, elaborated_body) =
                    self.solve_checking(extended_ctx, expr, body_type)?;

                let result_type = expected_type.clone();
                let elaborated = elab_node(
                    TermExpr::TyAbs(quantifier.clone(), kind.clone(), Box::new(elaborated_body)),
                    span,
                    result_type,
                );
                Ok((delta.until(&universal_entry), elaborated))
            }
            (_, TypeExpr::Qualified(preds, body)) => {
                let preds = preds.clone();
                self.with_givens(preds, |checker| checker.solve_checking(ctx, expr, body))
            }

            _ => {
                let (inferred_ctx, elaborated_expr, inferred_type) =
                    self.solve_inference(ctx, expr)?;
                let inferred_applied = inferred_ctx.apply(&inferred_type);
                let expected_applied = inferred_ctx.apply(expected_type);
                let delta = self.solve_subtype(
                    inferred_ctx,
                    &inferred_applied,
                    &expected_applied,
                    self.get_file_span(span),
                )?;
                Ok((delta, elaborated_expr))
            }
        }
    }

    fn infer_lambda_param_type(
        &mut self,
        ctx: Context,
        param: &str,
        maybe_annotation: &Option<TypeExpr>,
        span: Span,
    ) -> TcResult<(Context, TypeExpr)> {
        match maybe_annotation {
            Some(annotation) => {
                if !ctx.type_well_formed(annotation) {
                    return Err(TypeError::IllFormedAnnotation {
                        span: self.get_file_span(span),
                        ty: annotation.pretty(),
                        target: format!("parameter `{}`", param),
                    });
                }
                Ok((ctx, annotation.clone()))
            }
            None => {
                let evar_name = self.fresh_evar_name();
                let new_ctx = ctx.add_entry(ContextEntry::ExVar(evar_name.clone(), Kind::Star));
                Ok((new_ctx, TypeExpr::TyExVar(evar_name)))
            }
        }
    }

    fn int_type(&self) -> TypeExpr {
        TypeExpr::TyCon("prelude.builtins.Int".to_string())
    }

    fn solve_unary_operator_inference(
        &mut self,
        ctx: Context,
        op: &UnOp,
        rhs: &TermExprNode<Span>,
        span: Span,
    ) -> TcResult<(Context, TermExprNode<TcMetadata, Span>, TypeExpr)> {
        let int_type = self.int_type();
        let (ctx, elaborated_rhs) = self.solve_checking(ctx, rhs, &int_type)?;

        let result_type = int_type.clone();
        let elaborated = elab_node(
            TermExpr::UnaryOperator(*op, Box::new(elaborated_rhs)),
            span,
            result_type.clone(),
        );

        Ok((ctx, elaborated, result_type))
    }

    fn solve_binary_operator_inference(
        &mut self,
        ctx: Context,
        op: &BinOp,
        lhs: &TermExprNode<Span>,
        rhs: &TermExprNode<Span>,
        span: Span,
    ) -> TcResult<(Context, TermExprNode<TcMetadata, Span>, TypeExpr)> {
        let int_type = self.int_type();

        let (ctx, elaborated_lhs) = self.solve_checking(ctx, lhs, &int_type)?;
        let (ctx, elaborated_rhs) = self.solve_checking(ctx, rhs, &int_type)?;

        let result_type = match op {
            BinOp::Equal
            | BinOp::Less
            | BinOp::Greater
            | BinOp::GreaterOrEqual
            | BinOp::LessOrEqual => self.bool_type(),
            _ => self.int_type(),
        };

        let elaborated = elab_node(
            TermExpr::BinaryOperator(*op, Box::new(elaborated_lhs), Box::new(elaborated_rhs)),
            span,
            result_type.clone(),
        );

        Ok((ctx, elaborated, result_type))
    }

    pub fn solve_inference(
        &mut self,
        ctx: Context,
        expr: &TermExprNode<Span>,
    ) -> TcResult<(Context, TermExprNode<TcMetadata, Span>, TypeExpr)> {
        let span = expr.anno.clone();

        match &expr.node {
            TermExpr::Unit => {
                let elaborated = elab_node(TermExpr::Unit, span, TypeExpr::TyUnit);
                Ok((ctx, elaborated, TypeExpr::TyUnit))
            }
            TermExpr::Var(name) => {
                if let Some(var_type) = self.lookup_local_term_var(&ctx, name) {
                    let var_type = ctx.apply(&var_type);
                    let elaborated = elab_node(TermExpr::Var(name.clone()), span, var_type.clone());
                    return Ok((ctx, elaborated, var_type));
                }

                if let Some(scheme) = self.func_schemes.get(name).cloned() {
                    let (ctx, preds, instantiated_ty) = self.instantiate_scheme(ctx, &scheme);
                    self.add_wanteds(preds, span.clone());

                    let instantiated_ty = ctx.apply(&instantiated_ty);
                    let elaborated =
                        elab_node(TermExpr::Var(name.clone()), span, instantiated_ty.clone());
                    return Ok((ctx, elaborated, instantiated_ty));
                }
                if let Some(method_scheme) = self.lookup_class_method_scheme(name).cloned() {
                    eprintln!("class method {} : {}", name, method_scheme.pretty());

                    let (ctx, preds, instantiated_ty) =
                        self.instantiate_scheme(ctx, &method_scheme);
                    eprintln!(
                        "instantiated {} => preds [{}], ty {}",
                        name,
                        preds
                            .iter()
                            .map(|p| p.pretty())
                            .collect::<Vec<_>>()
                            .join(", "),
                        instantiated_ty.pretty()
                    );

                    self.add_wanteds(preds, span.clone());

                    let instantiated_ty = ctx.apply(&instantiated_ty);
                    let elaborated =
                        elab_node(TermExpr::Var(name.clone()), span, instantiated_ty.clone());
                    return Ok((ctx, elaborated, instantiated_ty));
                }

                Err(TypeError::UnboundVariable {
                    span: self.get_file_span(span.clone()),
                    name: name.clone(),
                })
            }
            TermExpr::Con(name) => {
                let constructor_type =
                    self.constructor_types.get(name).cloned().ok_or_else(|| {
                        TypeError::UnknownConstructor {
                            span: self.get_file_span(span.clone()),
                            name: name.clone(),
                        }
                    })?;
                let elaborated =
                    elab_node(TermExpr::Con(name.clone()), span, constructor_type.clone());
                Ok((ctx, elaborated, constructor_type))
            }
            TermExpr::Abs(param, maybe_annotation, body) => {
                let (ctx, param_type) =
                    self.infer_lambda_param_type(ctx, param, maybe_annotation, span.clone())?;

                let result_evar = self.fresh_evar_name();
                let typed_var_entry = ContextEntry::TypedVar {
                    var_name: param.clone(),
                    ty: param_type.clone(),
                };

                let extended_ctx = ctx
                    .add_entry(ContextEntry::ExVar(result_evar.clone(), Kind::Star))
                    .add_entry(typed_var_entry.clone());

                let body_expected_type = TypeExpr::TyExVar(result_evar.clone());
                let (delta, elaborated_body) =
                    self.solve_checking(extended_ctx, body, &body_expected_type)?;

                let final_param_type = delta.apply(&param_type);
                let final_result_type = delta.apply(&body_expected_type);
                let arrow_type =
                    TypeExpr::make_arr(final_param_type.clone(), final_result_type.clone());

                let elaborated = elab_node(
                    TermExpr::Abs(
                        param.clone(),
                        Some(final_param_type),
                        Box::new(elaborated_body),
                    ),
                    span,
                    arrow_type.clone(),
                );

                Ok((delta.until(&typed_var_entry), elaborated, arrow_type))
            }
            TermExpr::Let(name, rhs, body) => {
                let (gamma, elaborated_rhs, rhs_type) = self.solve_inference(ctx, rhs)?;
                let rhs_type = gamma.apply(&rhs_type);

                let typed_var_entry = ContextEntry::TypedVar {
                    var_name: name.clone(),
                    ty: rhs_type,
                };
                let extended_ctx = gamma.add_entry(typed_var_entry.clone());

                let (delta, elaborated_body, body_type) =
                    self.solve_inference(extended_ctx, body)?;
                let result_type = delta.apply(&body_type);

                let elaborated = elab_node(
                    TermExpr::Let(
                        name.clone(),
                        Box::new(elaborated_rhs),
                        Box::new(elaborated_body),
                    ),
                    span,
                    result_type.clone(),
                );

                Ok((delta.until(&typed_var_entry), elaborated, result_type))
            }
            TermExpr::App(function, argument) => {
                let (gamma, elaborated_function, function_type) =
                    self.solve_inference(ctx, function)?;
                let function_type_applied = gamma.apply(&function_type);
                self.solve_app_inference(
                    gamma,
                    &function_type_applied,
                    argument,
                    &elaborated_function,
                    function.anno.clone(),
                    argument.anno.clone(),
                )
            }
            TermExpr::Anno(inner, annotation_type) => {
                let (delta, elaborated_inner) = self.solve_checking(ctx, inner, annotation_type)?;
                let result_type = delta.apply(annotation_type);
                let elaborated = elab_node(
                    TermExpr::Anno(Box::new(elaborated_inner), result_type.clone()),
                    span,
                    result_type.clone(),
                );
                Ok((delta, elaborated, result_type))
            }
            TermExpr::TyAbs(param, kind, body) => {
                let universal_entry = ContextEntry::UniVar(param.clone(), kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());

                let (delta, elaborated_body, body_type) =
                    self.solve_inference(extended_ctx, body)?;

                let forall_type = TypeExpr::make_forall(param.clone(), kind.clone(), body_type);
                let elaborated = elab_node(
                    TermExpr::TyAbs(param.clone(), kind.clone(), Box::new(elaborated_body)),
                    span,
                    forall_type.clone(),
                );
                Ok((delta.until(&universal_entry), elaborated, forall_type))
            }
            TermExpr::TyApp(inner, ty_arg) => {
                let (delta, elaborated_inner, inner_type) = self.solve_inference(ctx, inner)?;
                let inner_type_applied = delta.apply(&inner_type);

                match inner_type_applied {
                    TypeExpr::TyForAll(quantifier, _kind, body) => {
                        let result_type = body.substitute(&quantifier, ty_arg);
                        let elaborated = elab_node(
                            TermExpr::TyApp(Box::new(elaborated_inner), ty_arg.clone()),
                            span,
                            result_type.clone(),
                        );
                        Ok((delta, elaborated, result_type))
                    }
                    other => Err(TypeError::NonPolymorphicTypeApplication {
                        span: self.get_file_span(span),
                        actual: other.pretty(),
                        ty_arg: ty_arg.pretty(),
                    }),
                }
            }
            TermExpr::Case(scrutinee, arms) => {
                self.solve_case_inference(ctx, scrutinee, arms, span)
            }

            TermExpr::IfThenElse(cond, then_branch, else_branch) => {
                let bool_type = self.bool_type();
                let (gamma, elaborated_cond) = self.solve_checking(ctx, cond, &bool_type)?;

                let result_evar = self.fresh_evar_name();
                let result_type = TypeExpr::TyExVar(result_evar.clone());
                let theta = gamma.add_entry(ContextEntry::ExVar(result_evar.clone(), Kind::Star));

                let (theta, elaborated_then) =
                    self.solve_checking(theta, then_branch, &result_type)?;
                let (theta, elaborated_else) =
                    self.solve_checking(theta, else_branch, &result_type)?;

                let final_type = theta.apply(&result_type);
                let elaborated = elab_node(
                    TermExpr::IfThenElse(
                        Box::new(elaborated_cond),
                        Box::new(elaborated_then),
                        Box::new(elaborated_else),
                    ),
                    span,
                    final_type.clone(),
                );
                Ok((theta, elaborated, final_type))
            }
            TermExpr::Lit(lit) => {
                let ty = match lit {
                    Literal::Int(_) => self.int_type(),
                };

                let elaborated = elab_node(TermExpr::Lit(lit.clone()), span, ty.clone());
                Ok((ctx, elaborated, ty))
            }
            TermExpr::BinaryOperator(op, lhs, rhs) => {
                self.solve_binary_operator_inference(ctx, op, lhs, rhs, span)
            }

            TermExpr::UnaryOperator(op, rhs) => {
                self.solve_unary_operator_inference(ctx, op, rhs, span)
            }
            TermExpr::Hole => {
                let evar_name = self.fresh_evar_name();
                let evar_type = TypeExpr::TyExVar(evar_name.clone());
                let extended_ctx = ctx.add_entry(ContextEntry::ExVar(evar_name, Kind::Star));
                self.report_hole(&extended_ctx, span.clone(), &evar_type);
                let elaborated = elab_node(TermExpr::Hole, span, evar_type.clone());
                Ok((extended_ctx, elaborated, evar_type))
            }
        }
    }

    fn solve_app_inference(
        &mut self,
        ctx: Context,
        function_type: &TypeExpr,
        argument: &TermExprNode<Span>,
        elaborated_function: &TermExprNode<TcMetadata, Span>,
        span: Span,
        arg_span: Span,
    ) -> TcResult<(Context, TermExprNode<TcMetadata, Span>, TypeExpr)> {
        let function_type = ctx.apply(function_type);

        match &function_type {
            TypeExpr::TyForAll(quantifier, kind, body) => {
                let fresh_evar = self.fresh_evar_name();
                let evar_entry = ContextEntry::ExVar(fresh_evar.clone(), kind.clone());
                let extended_ctx = ctx.add_entry(evar_entry);

                let ty_arg = TypeExpr::TyExVar(fresh_evar.clone());
                let instantiated_type = body.substitute(quantifier, &ty_arg);

                let wrapped_function = TermExprNode::new(
                    TermExpr::TyApp(Box::new(elaborated_function.clone()), ty_arg),
                    TcMetadata::new(span.clone(), instantiated_type.clone()),
                );

                self.solve_app_inference(
                    extended_ctx,
                    &instantiated_type,
                    argument,
                    &wrapped_function,
                    span,
                    arg_span,
                )
            }
            TypeExpr::TyArr(domain, codomain) => {
                let (delta, elaborated_argument) = self.solve_checking(ctx, argument, domain)?;

                let result_type = delta.apply(codomain);
                let elaborated = elab_node(
                    TermExpr::App(
                        Box::new(elaborated_function.clone()),
                        Box::new(elaborated_argument),
                    ),
                    span,
                    result_type.clone(),
                );
                Ok((delta, elaborated, result_type))
            }

            TypeExpr::TyExVar(evar_name) if ctx.has_unsolved_evar(evar_name) => {
                let evar_entry = ContextEntry::ExVar(
                    evar_name.clone(),
                    ctx.evar_kind(evar_name)
                        .unwrap_or_else(|| panic!("missing evar kind for {}", evar_name)),
                );
                let (mut left_entries, _, right_entries) = ctx.split_at(&evar_entry);

                let domain_evar = self.fresh_evar_name();
                let codomain_evar = self.fresh_evar_name();
                let arrow_solution = ContextEntry::SolvedEVar {
                    lhs: evar_name.clone(),
                    rhs: TypeExpr::make_arr(
                        TypeExpr::TyExVar(domain_evar.clone()),
                        TypeExpr::TyExVar(codomain_evar.clone()),
                    ),
                };
                self.record_solution(&arrow_solution);

                left_entries.push(ContextEntry::ExVar(codomain_evar.clone(), Kind::Star));
                left_entries.push(ContextEntry::ExVar(domain_evar.clone(), Kind::Star));
                left_entries.push(arrow_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);

                let (delta, elaborated_argument) =
                    self.solve_checking(new_ctx, argument, &TypeExpr::TyExVar(domain_evar))?;

                let result_type = delta.apply(&TypeExpr::TyExVar(codomain_evar));
                let elaborated = elab_node(
                    TermExpr::App(
                        Box::new(elaborated_function.clone()),
                        Box::new(elaborated_argument),
                    ),
                    span,
                    result_type.clone(),
                );
                Ok((delta, elaborated, result_type))
            }

            other => Err(TypeError::NonFunctionApplication {
                span: self.get_file_span(span.clone()),
                arg_span: self.get_file_span(arg_span.clone()),
                actual: other.pretty(),
            }),
        }
    }
}

pub type CheckedModule = ResolvedModule<TcMetadata, Span>;

pub struct TCOutput {
    pub module: CheckedModule,
    pub holes: Vec<HoleInfo>,
}

fn lift_span_expr_to_tcmeta(expr: &TermExprNode<Span, Span>) -> TermExprNode<TcMetadata, Span> {
    let anno = TcMetadata {
        span: expr.anno.clone(),
        inferred_type: TypeExpr::TyUnit,
    };

    let node = match &expr.node {
        TermExpr::Var(name) => TermExpr::Var(name.clone()),
        TermExpr::Con(name) => TermExpr::Con(name.clone()),
        TermExpr::Unit => TermExpr::Unit,
        TermExpr::Lit(lit) => TermExpr::Lit(lit.clone()),
        TermExpr::Hole => TermExpr::Hole,

        TermExpr::Abs(param, ann, body) => TermExpr::Abs(
            param.clone(),
            ann.clone(),
            Box::new(lift_span_expr_to_tcmeta(body)),
        ),

        TermExpr::App(f, x) => TermExpr::App(
            Box::new(lift_span_expr_to_tcmeta(f)),
            Box::new(lift_span_expr_to_tcmeta(x)),
        ),

        TermExpr::TyAbs(name, kind, body) => TermExpr::TyAbs(
            name.clone(),
            kind.clone(),
            Box::new(lift_span_expr_to_tcmeta(body)),
        ),

        TermExpr::TyApp(inner, ty) => {
            TermExpr::TyApp(Box::new(lift_span_expr_to_tcmeta(inner)), ty.clone())
        }

        TermExpr::Anno(inner, ty) => {
            TermExpr::Anno(Box::new(lift_span_expr_to_tcmeta(inner)), ty.clone())
        }

        TermExpr::Case(scrutinee, arms) => TermExpr::Case(
            Box::new(lift_span_expr_to_tcmeta(scrutinee)),
            arms.iter()
                .map(|arm| {
                    CaseArmNode::new(
                        CaseArm {
                            pattern: arm.node.pattern.clone(),
                            expr: lift_span_expr_to_tcmeta(&arm.node.expr),
                        },
                        arm.anno.clone(),
                    )
                })
                .collect(),
        ),

        TermExpr::IfThenElse(c, t, e) => TermExpr::IfThenElse(
            Box::new(lift_span_expr_to_tcmeta(c)),
            Box::new(lift_span_expr_to_tcmeta(t)),
            Box::new(lift_span_expr_to_tcmeta(e)),
        ),

        TermExpr::BinaryOperator(op, l, r) => TermExpr::BinaryOperator(
            *op,
            Box::new(lift_span_expr_to_tcmeta(l)),
            Box::new(lift_span_expr_to_tcmeta(r)),
        ),

        TermExpr::UnaryOperator(op, rhs) => {
            TermExpr::UnaryOperator(*op, Box::new(lift_span_expr_to_tcmeta(rhs)))
        }

        TermExpr::Let(name, binder, in_expr) => TermExpr::Let(
            name.clone(),
            Box::new(lift_span_expr_to_tcmeta(binder)),
            Box::new(lift_span_expr_to_tcmeta(in_expr)),
        ),
    };

    TermExprNode::new(node, anno)
}

fn lift_instance_defs_to_tcmeta(
    instance_defs: &[InstanceDefinition<Span, Span>],
) -> Vec<InstanceDefinition<TcMetadata, Span>> {
    instance_defs
        .iter()
        .map(|inst| InstanceDefinition {
            context: inst.context.clone(),
            head: inst.head.clone(),
            methods: inst
                .methods
                .iter()
                .map(|m| InstanceMethodDef {
                    name: m.name.clone(),
                    body: lift_span_expr_to_tcmeta(&m.body),
                })
                .collect(),
        })
        .collect()
}

pub fn check_and_elab_module(
    module: &ResolvedModule<Span>,
    file_id: impl Into<String>,
) -> Result<TCOutput, Vec<TypeError>> {
    let file_id = file_id.into();
    let mut errors = Vec::new();
    let mut checked_defs = Vec::new();
    let mut available_func_schemes = module.func_schemes.clone();
    let mut all_holes = Vec::new();

    for func_def in &module.func_defs {
        let mut checker = TypeChecker::with_module_context(module, file_id.clone());
        checker.func_schemes = available_func_schemes.clone();

        match &func_def.signature {
            Some(sig) => {
                let expected_type = TypeChecker::scheme_to_type(sig);

                match checker.solve_checking(Context::new(), &func_def.body, &expected_type) {
                    Ok((final_ctx, elaborated)) => {
                        let unresolved = checker.unresolved_wanteds();
                        if !unresolved.is_empty() {
                            errors.push(checker.wanteds_error(unresolved));
                        } else {
                            let zonked_expr = checker.zonk_expr(&elaborated);
                            let final_scheme = checker.zonk_scheme(sig);

                            checked_defs.push(TermDefinition {
                                name: func_def.name.clone(),
                                signature: Some(final_scheme.clone()),
                                body: zonked_expr,
                            });

                            available_func_schemes.insert(func_def.name.clone(), final_scheme);
                        }
                    }
                    Err(err) => {
                        errors.push(err);
                    }
                }
            }

            None => match checker.solve_inference(Context::new(), &func_def.body) {
                Ok((final_ctx, elaborated, inferred_type)) => {
                    let unresolved = checker.unresolved_wanteds();
                    if !unresolved.is_empty() {
                        errors.push(checker.wanteds_error(unresolved));
                    } else {
                        let zonked_expr = checker.zonk_expr(&elaborated);
                        let final_sig =
                            checker.generalize_top_level_type(&final_ctx, &inferred_type);
                        let final_scheme = TypeChecker::type_to_scheme(&final_sig);

                        checked_defs.push(TermDefinition {
                            name: func_def.name.clone(),
                            signature: Some(final_scheme.clone()),
                            body: zonked_expr,
                        });

                        available_func_schemes.insert(func_def.name.clone(), final_scheme);
                    }
                }
                Err(err) => {
                    errors.push(err);
                }
            },
        }

        checker.finalize_holes();
        all_holes.extend(checker.holes);
    }

    if errors.is_empty() {
        Ok(TCOutput {
            module: ResolvedModule {
                data_defs: module.data_defs.clone(),
                class_defs: module.class_defs.clone(),
                instance_defs: lift_instance_defs_to_tcmeta(&module.instance_defs.clone()),
                tycon_kinds: module.tycon_kinds.clone(),
                constructor_types: module.constructor_types.clone(),
                func_schemes: available_func_schemes,
                func_defs: checked_defs,
            },
            holes: all_holes,
        })
    } else {
        Err(errors)
    }
}
