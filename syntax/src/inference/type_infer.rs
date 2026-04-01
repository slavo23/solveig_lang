use std::collections::HashMap;

use crate::inference::inference_ctx::{Context, ContextEntry};
use base::utils::diagnostics::{FileSpan, TypeError};
use base::utils::span::Span;
use core::ast::{CaseArm, Kind, Pattern, Pretty, ResolvedModule, TermExpr, TermExprNode, TypeExpr};

#[derive(Clone, Debug)]
pub struct TcMetadata {
    pub span: Span,
    pub inferred_type: TypeExpr,
}

type TcResult<T> = Result<T, TypeError>;

impl TcMetadata {
    pub fn new(span: Span, inferred_type: TypeExpr) -> Self {
        Self {
            span,
            inferred_type,
        }
    }
}

fn elab_node(node: TermExpr<TcMetadata>, span: Span, ty: TypeExpr) -> TermExprNode<TcMetadata> {
    TermExprNode::new(node, TcMetadata::new(span, ty))
}

pub struct TypeChecker {
    file_id: String,
    counter: usize,
    constructor_types: HashMap<String, TypeExpr>,
    func_types: HashMap<String, TypeExpr>,
    tycon_kinds: HashMap<String, Kind>,
    evar_solutions: HashMap<String, TypeExpr>,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            file_id: "todo_file.sol".into(),
            counter: 0,
            constructor_types: HashMap::new(),
            func_types: HashMap::new(),
            tycon_kinds: HashMap::new(),
            evar_solutions: HashMap::new(),
        }
    }

    pub fn with_module_context(module: &ResolvedModule<Span>, file_id: String) -> Self {
        Self {
            file_id: file_id,
            counter: 0,
            constructor_types: module.constructor_types.clone(),
            func_types: module.func_types.clone(),
            tycon_kinds: module.tycon_kinds.clone(),
            evar_solutions: HashMap::new(),
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
        format!("{}${}", base, self.fresh_id())
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
            _ => ty.clone(),
        }
    }

    pub fn zonk_expr(&self, expr: &TermExprNode<TcMetadata>) -> TermExprNode<TcMetadata> {
        let zonked_meta = TcMetadata::new(
            expr.anno.span.clone(),
            self.zonk_type(&expr.anno.inferred_type),
        );
        let zonked_node = match &expr.node {
            TermExpr::Unit => TermExpr::Unit,
            TermExpr::Var(name) => TermExpr::Var(name.clone()),
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
                    .map(|arm| CaseArm {
                        pattern: arm.pattern.clone(),
                        expr: self.zonk_expr(&arm.expr),
                    })
                    .collect(),
            ),
            TermExpr::IfThenElse(cond, then_branch, else_branch) => TermExpr::IfThenElse(
                Box::new(self.zonk_expr(cond)),
                Box::new(self.zonk_expr(then_branch)),
                Box::new(self.zonk_expr(else_branch)),
            ),
        };
        TermExprNode::new(zonked_node, zonked_meta)
    }

    fn lookup_term_var(&self, ctx: &Context, name: &str) -> Option<TypeExpr> {
        ctx.lookup_term_var(name)
            .or_else(|| self.func_types.get(name).cloned())
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

        // Reflexivity
        if left == right {
            return Ok(ctx);
        }

        match (left, right) {
            (TypeExpr::TyUnit, TypeExpr::TyUnit) => Ok(ctx),
            (TypeExpr::TyVar(name_a), TypeExpr::TyVar(name_b)) if name_a == name_b => Ok(ctx),
            (TypeExpr::TyExVar(name_a), TypeExpr::TyExVar(name_b)) if name_a == name_b => Ok(ctx),
            (TypeExpr::TyCon(name_a), TypeExpr::TyCon(name_b)) if name_a == name_b => Ok(ctx),
            (TypeExpr::TyArr(domain_a, codomain_a), TypeExpr::TyArr(domain_b, codomain_b)) => {
                let theta = self.solve_subtype_inner(ctx, domain_b, domain_a)?;
                let codomain_a_applied = theta.apply(codomain_a);
                let codomain_b_applied = theta.apply(codomain_b);
                self.solve_subtype_inner(
                    theta,
                    &codomain_a_applied,
                    &codomain_b_applied,
                )
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
                let theta =
                    self.solve_subtype_inner(extended_ctx, &instantiated_body, right)?;
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
                self.instantiate_left(ctx, &evar_name, right)
            }
            (_, TypeExpr::TyExVar(evar_name))
                if ctx.has_unsolved_evar(evar_name) && !left.occurs_check(evar_name.clone()) =>
            {
                self.instantiate_right(ctx, left, &evar_name)
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
            // InstLReach: α̂ :=< β̂  where α̂ is before β̂
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

            // InstLReach (reversed): β̂ before α̂ → solve α̂ = β̂
            TypeExpr::TyExVar(other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: evar_name.to_string(),
                    rhs: TypeExpr::TyExVar(other_evar.clone()),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(&evar_entry, &solution))
            }

            // InstLArr: α̂ :=< A → B
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

                // Insert: ..., α̂₂, α̂₁, α̂ = α̂₁ → α̂₂, ...
                left_entries.push(ContextEntry::ExVar(codomain_evar.clone(), Kind::Star));
                left_entries.push(ContextEntry::ExVar(domain_evar.clone(), Kind::Star));
                left_entries.push(arrow_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);

                // Contravariant: B₁ =:> α̂₁
                let theta = self.instantiate_right(new_ctx, domain, &domain_evar)?;
                // Covariant: α̂₂ :=< [Θ]B₂
                let codomain_applied = theta.apply(codomain);
                self.instantiate_left(theta, &codomain_evar, &codomain_applied)
            }

            // InstLApp: α̂ :=< F X
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

            // InstLAllR: α̂ :=< ∀b. B
            TypeExpr::TyForAll(quantifier, kind, body) => {
                let universal_entry = ContextEntry::UniVar(quantifier.clone(), kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());
                let delta = self.instantiate_left(extended_ctx, evar_name, body)?;
                Ok(delta.until(&universal_entry))
            }

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

        // InstRSolve: if source is a well-formed monotype in the left context
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
            // InstRReach: β̂ =:> α̂  (earlier solves to later)
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

            // InstRArr: A → B =:> α̂
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

                // Note: flipped direction compared to InstLArr
                let theta = self.instantiate_left(new_ctx, &domain_evar, domain)?;
                let codomain_applied = theta.apply(codomain);
                self.instantiate_right(theta, &codomain_applied, &codomain_evar)
            }

            // InstRApp: F X =:> α̂
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

            // InstRAllL: ∀b. B =:> α̂
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
                let instantiated = body.substitute(&quantifier, &TypeExpr::TyExVar(fresh_evar));
                self.force_arrow(new_ctx, &instantiated)
            }

            TypeExpr::TyExVar(evar_name) if ctx.has_unsolved_evar(evar_name) => {
                let evar_entry = ContextEntry::ExVar(evar_name.clone(), Kind::Star);
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

    fn check_pattern(
        &mut self,
        ctx: Context,
        pattern: &Pattern,
        scrutinee_type: &TypeExpr,
    ) -> TcResult<(Context, Vec<(String, TypeExpr)>)> {
        let scrutinee_type = ctx.apply(scrutinee_type);

        match pattern {
            Pattern::Wild => Ok((ctx, vec![])),

            Pattern::Var(name) => Ok((ctx, vec![(name.clone(), scrutinee_type)])),

            Pattern::Con(constructor_name, sub_patterns) => {
                let constructor_type = self
                    .constructor_types
                    .get(constructor_name)
                    .cloned()
                    .ok_or_else(|| TypeError::UnknownPatternConstructor {
                        span: self.get_file_span(0..0),
                        name: constructor_name.clone(),
                    })?;

                let mut theta = ctx;
                let mut current_type = constructor_type;
                let mut field_types = Vec::with_capacity(sub_patterns.len());

                for _ in 0..sub_patterns.len() {
                    let (new_ctx, domain, codomain) = self.force_arrow(theta, &current_type)?;
                    theta = new_ctx;
                    field_types.push(domain);
                    current_type = codomain;
                }

                // Subtype the constructor's result type against scrutinee
                let result_type_applied = theta.apply(&current_type);
                let scrutinee_applied = theta.apply(&scrutinee_type);
                theta = self.solve_subtype(
                    theta,
                    &result_type_applied,
                    &scrutinee_applied,
                    self.get_file_span(0..0),
                )?;

                // Recurse into sub-patterns
                let mut all_bindings = Vec::new();
                for (sub_pattern, field_type) in sub_patterns.iter().zip(field_types.iter()) {
                    let field_type_applied = theta.apply(field_type);
                    let (new_ctx, bindings) =
                        self.check_pattern(theta, sub_pattern, &field_type_applied)?;
                    theta = new_ctx;
                    all_bindings.extend(bindings);
                }

                Ok((theta, all_bindings))
            }
        }
    }

    pub fn solve_checking(
        &mut self,
        ctx: Context,
        expr: &TermExprNode<Span>,
        expected_type: &TypeExpr,
    ) -> TcResult<(Context, TermExprNode<TcMetadata>)> {
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
                let ctx = if let Some(annotation) = maybe_annotation {
                    let annotation_applied = ctx.apply(annotation);
                    let domain_applied = ctx.apply(domain);
                    let ctx = self.solve_subtype(
                        ctx,
                        &domain_applied,
                        &annotation_applied,
                        self.get_file_span(span.clone()),
                    )?;
                    self.solve_subtype(
                        ctx.clone(),
                        &ctx.apply(annotation),
                        &ctx.apply(domain),
                        self.get_file_span(span.clone()),
                    )?
                } else {
                    ctx
                };

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

            (TermExpr::IfThenElse(cond, then_branch, else_branch), _) => {
                let bool_type = TypeExpr::TyCon("Bool".to_string());
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

    pub fn solve_inference(
        &mut self,
        ctx: Context,
        expr: &TermExprNode<Span>,
    ) -> TcResult<(Context, TermExprNode<TcMetadata>, TypeExpr)> {
        let span = expr.anno.clone();

        match &expr.node {
            // Unit⇒
            TermExpr::Unit => {
                let elaborated = elab_node(TermExpr::Unit, span, TypeExpr::TyUnit);
                Ok((ctx, elaborated, TypeExpr::TyUnit))
            }

            // Var⇒
            TermExpr::Var(name) => {
                let var_type =
                    self.lookup_term_var(&ctx, name)
                        .ok_or_else(|| TypeError::UnboundVariable {
                            span: self.get_file_span(span.clone()),
                            name: name.clone(),
                        })?;
                let var_type = ctx.apply(&var_type);
                let elaborated = elab_node(TermExpr::Var(name.clone()), span, var_type.clone());
                Ok((ctx, elaborated, var_type))
            }

            // Con⇒
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

            // →I⇒: λx. e  (infer mode — create evars for param and return)
            TermExpr::Abs(param, maybe_annotation, body) => {
                let (param_type, ctx) = match maybe_annotation {
                    Some(annotation) => (annotation.clone(), ctx),
                    None => {
                        let evar_name = self.fresh_evar_name();
                        let new_ctx =
                            ctx.add_entry(ContextEntry::ExVar(evar_name.clone(), Kind::Star));
                        (TypeExpr::TyExVar(evar_name), new_ctx)
                    }
                };

                let return_evar = self.fresh_evar_name();
                let typed_var_entry = ContextEntry::TypedVar {
                    var_name: param.clone(),
                    ty: param_type.clone(),
                };
                let extended_ctx = ctx
                    .add_entry(ContextEntry::ExVar(return_evar.clone(), Kind::Star))
                    .add_entry(typed_var_entry.clone());

                let return_type = TypeExpr::TyExVar(return_evar.clone());
                let (delta, elaborated_body) =
                    self.solve_checking(extended_ctx, body, &return_type)?;

                let arrow_type =
                    TypeExpr::make_arr(delta.apply(&param_type), delta.apply(&return_type));
                let elaborated = elab_node(
                    TermExpr::Abs(
                        param.clone(),
                        Some(delta.apply(&param_type)),
                        Box::new(elaborated_body),
                    ),
                    span,
                    arrow_type.clone(),
                );
                Ok((delta.until(&typed_var_entry), elaborated, arrow_type))
            }

            // →E⇒: e₁ e₂  (application — infer function, then app-infer)
            TermExpr::App(function, argument) => {
                let (gamma, elaborated_function, function_type) =
                    self.solve_inference(ctx, function)?;
                let function_type_applied = gamma.apply(&function_type);
                self.solve_app_inference(
                    gamma,
                    &function_type_applied,
                    argument,
                    &elaborated_function,
                    span,
                )
            }

            // Anno⇒: (e : A)
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

            // TyAbs⇒: Λα. e
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

            // TyApp⇒: e @A
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

            // Case⇒
            TermExpr::Case(scrutinee, arms) => {
                if arms.is_empty() {
                    todo!()
                }

                let (gamma, elaborated_scrutinee, scrutinee_type) =
                    self.solve_inference(ctx, scrutinee)?;
                let scrutinee_type = gamma.apply(&scrutinee_type);

                let result_evar = self.fresh_evar_name();
                let result_type = TypeExpr::TyExVar(result_evar.clone());
                let mut theta =
                    gamma.add_entry(ContextEntry::ExVar(result_evar.clone(), Kind::Star));

                let mut elaborated_arms = Vec::with_capacity(arms.len());

                for (arm_index, arm) in arms.iter().enumerate() {
                    let arm_marker = ContextEntry::Marker {
                        var_name: format!("case_arm#{}", arm_index),
                    };
                    let arm_ctx = theta.clone().add_entry(arm_marker.clone());
                    let scrutinee_in_arm = arm_ctx.apply(&scrutinee_type);

                    let (delta, bindings) =
                        self.check_pattern(arm_ctx, &arm.pattern, &scrutinee_in_arm)?;

                    let mut body_ctx = delta;
                    for (var_name, var_type) in &bindings {
                        body_ctx = body_ctx.add_entry(ContextEntry::TypedVar {
                            var_name: var_name.clone(),
                            ty: var_type.clone(),
                        });
                    }

                    let arm_result_type = body_ctx.apply(&result_type);
                    let (omega, elaborated_body) =
                        self.solve_checking(body_ctx, &arm.expr, &arm_result_type)?;
                    theta = omega.until(&arm_marker);

                    elaborated_arms.push(CaseArm {
                        pattern: arm.pattern.clone(),
                        expr: elaborated_body,
                    });
                }

                let final_type = theta.apply(&result_type);
                let elaborated = elab_node(
                    TermExpr::Case(Box::new(elaborated_scrutinee), elaborated_arms),
                    span,
                    final_type.clone(),
                );
                Ok((theta, elaborated, final_type))
            }

            // If⇒
            TermExpr::IfThenElse(cond, then_branch, else_branch) => {
                let bool_type = TypeExpr::TyCon("Bool".to_string());
                let (gamma, elaborated_cond, cond_type) = self.solve_inference(ctx, cond)?;
                let gamma = self.solve_subtype(
                    gamma.clone(),
                    &gamma.apply(&cond_type),
                    &bool_type,
                    self.get_file_span(cond.anno.clone()),
                )?;

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
        }
    }

    fn solve_app_inference(
        &mut self,
        ctx: Context,
        function_type: &TypeExpr,
        argument: &TermExprNode<Span>,
        elaborated_function: &TermExprNode<TcMetadata>,
        span: Span,
    ) -> TcResult<(Context, TermExprNode<TcMetadata>, TypeExpr)> {
        let function_type = ctx.apply(function_type);

        match &function_type {
            TypeExpr::TyForAll(quantifier, kind, body) => {
                let fresh_evar = self.fresh_evar_name();
                let evar_entry = ContextEntry::ExVar(fresh_evar.clone(), kind.clone());
                let extended_ctx = ctx.add_entry(evar_entry);

                let ty_arg = TypeExpr::TyExVar(fresh_evar.clone());
                let instantiated_type = body.substitute(&quantifier, &ty_arg);

                // Wrap function in TyApp for elaboration
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
                )
            }

            // →App: function is A → B — check argument against A
            TypeExpr::TyArr(domain, codomain) => {
                let (delta, elaborated_argument) = self.solve_checking(ctx, argument, &domain)?;

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
                let evar_entry = ContextEntry::ExVar(evar_name.clone(), Kind::Star);
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
                actual: other.pretty(),
            }),
        }
    }
}

pub fn check_and_elab_module(
    module: &ResolvedModule<Span>,
    file_id: impl Into<String>,
) -> Result<(), Vec<TypeError>> {
    let file_id = file_id.into();
    let mut errors = Vec::new();

    for func_def in &module.func_defs {
        let mut inference = TypeChecker::with_module_context(module, file_id.clone());

        match inference.solve_checking(Context::new(), &func_def.body, &func_def.signature) {
            Ok((_ctx, elaborated_expr)) => {
                let _zonked = inference.zonk_expr(&elaborated_expr);
                println!("(OK) {}: {}", func_def.name, func_def.signature.pretty());
            }
            Err(err) => {
                println!("(ERR) {} : ???", func_def.name);
                errors.push(err);
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
