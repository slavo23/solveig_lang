impl TypeChecker<'_> {
    fn lookup_local_term_var(&self, ctx: &Context, name: &str) -> Option<Ty> {
        ctx.lookup_term_var(name)
    }
    pub fn solve_subtype(
        &mut self,
        ctx: Context,
        left: &Ty,
        right: &Ty,
        origin_span: FileSpan,
    ) -> Result<Context, TypeError> {
        let original_left = ctx.apply(left);
        let original_right = ctx.apply(right);

        match self.solve_subtype_inner(
            ctx,
            &original_left,
            &original_right,
            origin_span.span.to_usize_range(),
        ) {
            Ok(delta) => Ok(delta),
            Err(()) => Err(TypeError::SubtypeMismatch {
                span: origin_span,
                left: self.pretty_type(left),
                right: self.pretty_type(right),
            }),
        }
    }
    fn solve_subtype_inner(
        &mut self,
        ctx: Context,
        left: &Ty,
        right: &Ty,
        origin_span: Span,
    ) -> Result<Context, ()> {
        // The context is ORDERED and that ordering is load-bearing: DK 2013
        // §4 permits solving an existential only to a type whose variables
        // appear EARLIER in the context. A union-find table has no notion of
        // scope, so replacing this with ena would silently admit exactly the
        // unsoundness the ordering prevents. Kind inference (kind_unify.rs) is
        // the only place plain HM unification is correct here.
        let left = &ctx.apply(left);
        let right = &ctx.apply(right);

        if left == right {
            return Ok(ctx);
        }

        // DK 2013, Fig. 9, <:forallL/<:forallR; B2 extends each opened
        // forall group with its qualified context instead of dropping it.
        match (left, right) {
            (Ty::TyUnit, Ty::TyUnit) => Ok(ctx),
            (Ty::TyVar(name_a), Ty::TyVar(name_b)) if name_a == name_b => Ok(ctx),
            (Ty::TyExVar(name_a), Ty::TyExVar(name_b)) if name_a == name_b => Ok(ctx),
            (Ty::TyCon(name_a), Ty::TyCon(name_b)) if name_a == name_b => Ok(ctx),

            (Ty::TyArr(domain_a, codomain_a), Ty::TyArr(domain_b, codomain_b)) => {
                let theta =
                    self.solve_subtype_inner(ctx, domain_b, domain_a, origin_span.clone())?;
                let codomain_a_applied = theta.apply(codomain_a);
                let codomain_b_applied = theta.apply(codomain_b);
                self.solve_subtype_inner(
                    theta,
                    &codomain_a_applied,
                    &codomain_b_applied,
                    origin_span,
                )
            }

            (Ty::TyApp(func_a, arg_a), Ty::TyApp(func_b, arg_b)) => {
                let theta = self.solve_subtype_inner(ctx, func_b, func_a, origin_span.clone())?;
                let arg_a_applied = theta.apply(arg_a);
                let arg_b_applied = theta.apply(arg_b);
                self.solve_subtype_inner(theta, &arg_a_applied, &arg_b_applied, origin_span)
            }

            (Ty::TyForAll(binders, preds, body), _) if !binders.is_empty() => {
                let Some(((quantifier, kind), rest_binders)) = binders.split_first() else {
                    solveig_base::ice!("non-empty forall binder guard failed");
                };
                let fresh_evar = self.fresh_evar_id();
                let marker = ContextEntry::Marker {
                    var_name: solveig_ast::ast::evar_display_name(fresh_evar),
                };
                let extended_ctx = ctx
                    .add_entry(marker.clone())
                    .add_entry(ContextEntry::ExVar(fresh_evar, kind.clone()));

                let ty_arg = Self::evar_type(fresh_evar);
                let subst = FxHashMap::from_iter([(*quantifier, ty_arg.clone())]);
                let instantiated_body = Ty::make_forall_group(
                    rest_binders.to_vec(),
                    preds
                        .iter()
                        .map(|pred| Self::substitute_predicate(pred, &subst))
                        .collect(),
                    body.substitute(*quantifier, &ty_arg),
                );
                let theta = self.solve_subtype_inner(
                    extended_ctx,
                    &instantiated_body,
                    right,
                    origin_span,
                )?;
                Ok(theta.until(&marker))
            }
            (Ty::TyForAll(_, preds, body), _) => {
                self.add_wanteds(preds.clone(), origin_span.clone());
                self.solve_subtype_inner(ctx, body, right, origin_span)
            }

            (_, Ty::TyForAll(binders, preds, body)) if !binders.is_empty() => {
                let Some(((quantifier, kind), rest_binders)) = binders.split_first() else {
                    solveig_base::ice!("non-empty forall binder guard failed");
                };
                let universal_entry = self.universal_entry(*quantifier, kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());
                let body = Ty::make_forall_group(
                    rest_binders.to_vec(),
                    preds.clone(),
                    (**body).clone(),
                );
                let theta =
                    self.solve_subtype_inner(extended_ctx, left, &body, origin_span)?;
                Ok(theta.until(&universal_entry))
            }
            (_, Ty::TyForAll(_, preds, body)) => {
                let preds = preds.clone();
                let (_, result) = self.with_givens(preds, |checker| {
                    checker.solve_subtype_inner(ctx, left, body, origin_span)
                });
                result
            }

            (Ty::TyExVar(evar_id), _)
                if ctx.has_unsolved_evar_id(*evar_id) && !right.occurs_check(*evar_id) =>
            {
                self.instantiate_left(ctx, *evar_id, right, origin_span)
            }

            (_, Ty::TyExVar(evar_id))
                if ctx.has_unsolved_evar_id(*evar_id) && !left.occurs_check(*evar_id) =>
            {
                self.instantiate_right(ctx, left, *evar_id, origin_span)
            }

            _ => Err(()),
        }
    }
    fn tyapp_split_kinds(
        &self,
        ctx: &Context,
        result_evar: EvarId,
        arg_part: &Ty,
    ) -> Result<(Kind, Kind), ()> {
        let arg_kind = ctx
            .kind_of_type(arg_part, self.tables.tycon_kinds)
            .map_err(|_error| ())?;

        let result_kind = ctx
            .evar_kind_id(result_evar)
            .unwrap_or_else(|| solveig_base::ice!("missing evar kind for ?{}", result_evar.0));

        let func_kind = Kind::make_arrow(arg_kind.clone(), result_kind);

        Ok((func_kind, arg_kind))
    }
    // DK 2013, Fig. 10, InstL/InstR; Solveig decomposes arrows and type
    // applications by solving the existential at its ordered context position.
    fn instantiate_left(
        &mut self,
        ctx: Context,
        evar_id: EvarId,
        target: &Ty,
        origin_span: Span,
    ) -> Result<Context, ()> {
        let evar_entry = ContextEntry::ExVar(
            evar_id,
            ctx.evar_kind_id(evar_id)
                .unwrap_or_else(|| solveig_base::ice!("missing evar kind for ?{}", evar_id.0)),
        );
        let (mut left_entries, _, right_entries) = ctx.split_at(&evar_entry);

        let left_ctx = Context::from_entries(left_entries.clone());
        if left_ctx.type_well_formed(target) && target.is_mono() {
            let solution = ContextEntry::SolvedEVar {
                lhs: evar_id,
                rhs: target.clone(),
            };
            self.record_solution(&solution);
            left_entries.push(solution);
            left_entries.extend(right_entries);
            return Ok(Context::from_entries(left_entries));
        }

        match target {
            Ty::TyExVar(other_evar) if ctx.evar_before_id(evar_id, *other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: *other_evar,
                    rhs: Self::evar_type(evar_id),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(
                    &ContextEntry::ExVar(
                        *other_evar,
                        ctx.evar_kind_id(*other_evar)
                            .unwrap_or_else(|| solveig_base::ice!("missing evar kind for ?{}", other_evar.0)),
                    ),
                    &solution,
                ))
            }
            Ty::TyExVar(other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: Self::evar_type(*other_evar),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(&evar_entry, &solution))
            }
            Ty::TyArr(domain, codomain) => {
                let domain_evar = self.fresh_evar_id();
                let codomain_evar = self.fresh_evar_id();
                let arrow_solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: Ty::make_arr(
                        Self::evar_type(domain_evar),
                        Self::evar_type(codomain_evar),
                    ),
                };
                self.record_solution(&arrow_solution);

                left_entries.push(ContextEntry::ExVar(codomain_evar, Kind::Star));
                left_entries.push(ContextEntry::ExVar(domain_evar, Kind::Star));
                left_entries.push(arrow_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);

                let theta =
                    self.instantiate_right(new_ctx, domain, domain_evar, origin_span.clone())?;
                let codomain_applied = theta.apply(codomain);
                self.instantiate_left(theta, codomain_evar, &codomain_applied, origin_span)
            }
            Ty::TyApp(func_part, arg_part) => {
                let (func_kind, arg_kind) = self.tyapp_split_kinds(&ctx, evar_id, arg_part)?;

                let func_evar = self.fresh_evar_id();
                let arg_evar = self.fresh_evar_id();

                let app_solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: Ty::make_app(
                        Self::evar_type(func_evar),
                        Self::evar_type(arg_evar),
                    ),
                };

                self.record_solution(&app_solution);

                left_entries.push(ContextEntry::ExVar(arg_evar, arg_kind));
                left_entries.push(ContextEntry::ExVar(func_evar, func_kind));
                left_entries.push(app_solution);

                let new_ctx = Context::from_halves(left_entries, right_entries);

                let theta =
                    self.instantiate_left(new_ctx, func_evar, func_part, origin_span.clone())?;
                let arg_applied = theta.apply(arg_part);

                self.instantiate_left(theta, arg_evar, &arg_applied, origin_span)
            }
            Ty::TyForAll(binders, preds, body) if !binders.is_empty() => {
                let Some(((quantifier, kind), rest_binders)) = binders.split_first() else {
                    solveig_base::ice!("non-empty forall binder guard failed");
                };
                let universal_entry = self.universal_entry(*quantifier, kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());
                let body = Ty::make_forall_group(
                    rest_binders.to_vec(),
                    preds.clone(),
                    (**body).clone(),
                );
                let delta = self.instantiate_left(extended_ctx, evar_id, &body, origin_span)?;
                Ok(delta.until(&universal_entry))
            }
            Ty::TyForAll(_, preds, body) => {
                let preds = preds.clone();
                let (_, result) = self.with_givens(preds, |checker| {
                    checker.instantiate_left(ctx, evar_id, body, origin_span)
                });
                result
            }

            _ => Err(()),
        }
    }
    fn instantiate_right(
        &mut self,
        ctx: Context,
        source: &Ty,
        evar_id: EvarId,
        origin_span: Span,
    ) -> Result<Context, ()> {
        let evar_entry = ContextEntry::ExVar(
            evar_id,
            ctx.evar_kind_id(evar_id)
                .unwrap_or_else(|| solveig_base::ice!("missing evar kind for ?{}", evar_id.0)),
        );
        let (mut left_entries, _, right_entries) = ctx.split_at(&evar_entry);
        let left_ctx = Context::from_entries(left_entries.clone());
        if left_ctx.type_well_formed(source) && source.is_mono() {
            let solution = ContextEntry::SolvedEVar {
                lhs: evar_id,
                rhs: source.clone(),
            };
            self.record_solution(&solution);
            left_entries.push(solution);
            left_entries.extend(right_entries);
            return Ok(Context::from_entries(left_entries));
        }

        match source {
            Ty::TyExVar(other_evar) if ctx.evar_before_id(evar_id, *other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: *other_evar,
                    rhs: Self::evar_type(evar_id),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(
                    &ContextEntry::ExVar(
                        *other_evar,
                        ctx.evar_kind_id(*other_evar)
                            .unwrap_or_else(|| solveig_base::ice!("missing evar kind for ?{}", other_evar.0)),
                    ),
                    &solution,
                ))
            }

            Ty::TyExVar(other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: Self::evar_type(*other_evar),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(&evar_entry, &solution))
            }
            Ty::TyArr(domain, codomain) => {
                let domain_evar = self.fresh_evar_id();
                let codomain_evar = self.fresh_evar_id();
                let arrow_solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: Ty::make_arr(
                        Self::evar_type(domain_evar),
                        Self::evar_type(codomain_evar),
                    ),
                };
                self.record_solution(&arrow_solution);

                left_entries.push(ContextEntry::ExVar(codomain_evar, Kind::Star));
                left_entries.push(ContextEntry::ExVar(domain_evar, Kind::Star));
                left_entries.push(arrow_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);
                let theta =
                    self.instantiate_left(new_ctx, domain_evar, domain, origin_span.clone())?;
                let codomain_applied = theta.apply(codomain);
                self.instantiate_right(theta, &codomain_applied, codomain_evar, origin_span)
            }
            Ty::TyApp(func_part, arg_part) => {
                let (func_kind, arg_kind) = self.tyapp_split_kinds(&ctx, evar_id, arg_part)?;

                let func_evar = self.fresh_evar_id();
                let arg_evar = self.fresh_evar_id();

                let app_solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: Ty::make_app(
                        Self::evar_type(func_evar),
                        Self::evar_type(arg_evar),
                    ),
                };

                self.record_solution(&app_solution);

                left_entries.push(ContextEntry::ExVar(arg_evar, arg_kind));
                left_entries.push(ContextEntry::ExVar(func_evar, func_kind));
                left_entries.push(app_solution);

                let new_ctx = Context::from_halves(left_entries, right_entries);

                let theta =
                    self.instantiate_right(new_ctx, func_part, func_evar, origin_span.clone())?;
                let arg_applied = theta.apply(arg_part);

                self.instantiate_right(theta, &arg_applied, arg_evar, origin_span)
            }
            Ty::TyForAll(binders, preds, body) if !binders.is_empty() => {
                let Some(((quantifier, kind), rest_binders)) = binders.split_first() else {
                    solveig_base::ice!("non-empty forall binder guard failed");
                };
                let fresh_evar = self.fresh_evar_id();
                let marker = ContextEntry::Marker {
                    var_name: solveig_ast::ast::evar_display_name(fresh_evar),
                };
                let extended_ctx = ctx
                    .add_entry(marker.clone())
                    .add_entry(ContextEntry::ExVar(fresh_evar, kind.clone()));

                let ty_arg = Self::evar_type(fresh_evar);
                let subst = FxHashMap::from_iter([(*quantifier, ty_arg.clone())]);
                let instantiated = Ty::make_forall_group(
                    rest_binders.to_vec(),
                    preds
                        .iter()
                        .map(|pred| Self::substitute_predicate(pred, &subst))
                        .collect(),
                    body.substitute(*quantifier, &ty_arg),
                );
                let delta =
                    self.instantiate_right(extended_ctx, &instantiated, evar_id, origin_span)?;
                Ok(delta.until(&marker))
            }
            Ty::TyForAll(_, preds, body) => {
                self.add_wanteds(preds.clone(), origin_span.clone());
                self.instantiate_right(ctx, body, evar_id, origin_span)
            }

            _ => Err(()),
        }
    }
    fn force_arrow(
        &mut self,
        ctx: Context,
        ty: &Ty,
        span: Span,
    ) -> TcResult<(Context, Ty, Ty)> {
        let ty = ctx.apply(ty);

        match &ty {
            Ty::TyArr(domain, codomain) => {
                Ok((ctx, (**domain).clone(), (**codomain).clone()))
            }

            Ty::TyForAll(binders, preds, body) if !binders.is_empty() => {
                let Some(((quantifier, kind), rest_binders)) = binders.split_first() else {
                    solveig_base::ice!("non-empty forall binder guard failed");
                };
                let fresh_evar = self.fresh_evar_id();
                let new_ctx = ctx.add_entry(ContextEntry::ExVar(fresh_evar, kind.clone()));
                let ty_arg = Self::evar_type(fresh_evar);
                let subst = FxHashMap::from_iter([(*quantifier, ty_arg.clone())]);
                let instantiated = Ty::make_forall_group(
                    rest_binders.to_vec(),
                    preds
                        .iter()
                        .map(|pred| Self::substitute_predicate(pred, &subst))
                        .collect(),
                    body.substitute(*quantifier, &ty_arg),
                );
                self.force_arrow(new_ctx, &instantiated, span)
            }
            Ty::TyForAll(_, preds, body) => {
                self.add_wanteds(preds.clone(), span.clone());
                self.force_arrow(ctx, body, span)
            }

            Ty::TyExVar(evar_id) if ctx.has_unsolved_evar_id(*evar_id) => {
                let evar_entry = ContextEntry::ExVar(
                    *evar_id,
                    ctx.evar_kind_id(*evar_id)
                        .unwrap_or_else(|| solveig_base::ice!("missing evar kind for ?{}", evar_id.0)),
                );
                let (mut left_entries, _, right_entries) = ctx.split_at(&evar_entry);

                let domain_evar = self.fresh_evar_id();
                let codomain_evar = self.fresh_evar_id();
                let arrow_solution = ContextEntry::SolvedEVar {
                    lhs: *evar_id,
                    rhs: Ty::make_arr(
                        Self::evar_type(domain_evar),
                        Self::evar_type(codomain_evar),
                    ),
                };
                self.record_solution(&arrow_solution);

                left_entries.push(ContextEntry::ExVar(codomain_evar, Kind::Star));
                left_entries.push(ContextEntry::ExVar(domain_evar, Kind::Star));
                left_entries.push(arrow_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);

                Ok((new_ctx, Self::evar_type(domain_evar), Self::evar_type(codomain_evar)))
            }

            _ => Err(TypeError::ConstructorNotApplicable {
                span: self.get_file_span(span),
                actual: self.pretty_type(&ty),
            }),
        }
    }
}
