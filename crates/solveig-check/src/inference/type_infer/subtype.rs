impl TypeChecker {
    fn lookup_local_term_var(&self, ctx: &Context, name: &str) -> Option<TypeExpr> {
        ctx.lookup_term_var(name)
            .or_else(|| self.constructor_types.get(&legacy_ctor_id(name)).cloned())
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
                let fresh_evar = self.fresh_evar_id();
                let marker = ContextEntry::Marker {
                    var_name: solveig_ast::ast::legacy_evar_name(fresh_evar),
                };
                let extended_ctx = ctx
                    .add_entry(marker.clone())
                    .add_entry(ContextEntry::ExVar(fresh_evar, kind.clone()));

                let instantiated_body =
                    body.substitute(*quantifier, &Self::evar_type(fresh_evar));
                let theta = self.solve_subtype_inner(extended_ctx, &instantiated_body, right)?;
                Ok(theta.until(&marker))
            }

            (_, TypeExpr::TyForAll(quantifier, kind, body)) => {
                let universal_entry = Self::universal_entry(*quantifier, kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());
                let theta = self.solve_subtype_inner(extended_ctx, left, body)?;
                Ok(theta.until(&universal_entry))
            }

            (TypeExpr::TyExVar(evar_id), _)
                if ctx.has_unsolved_evar_id(*evar_id) && !right.occurs_check(*evar_id) =>
            {
                self.instantiate_left(ctx, *evar_id, right)
            }

            (_, TypeExpr::TyExVar(evar_id))
                if ctx.has_unsolved_evar_id(*evar_id) && !left.occurs_check(*evar_id) =>
            {
                self.instantiate_right(ctx, left, *evar_id)
            }

            _ => Err(()),
        }
    }
    fn tyapp_split_kinds(
        &self,
        ctx: &Context,
        result_evar: EvarId,
        arg_part: &TypeExpr,
    ) -> Result<(Kind, Kind), ()> {
        let arg_kind = ctx
            .kind_of_type(arg_part, &self.tycon_kinds)
            .map_err(|_| ())?;

        let result_kind = ctx
            .evar_kind_id(result_evar)
            .unwrap_or_else(|| panic!("ICE: missing evar kind for ?{}", result_evar.0));

        let func_kind = Kind::make_arrow(arg_kind.clone(), result_kind);

        Ok((func_kind, arg_kind))
    }
    fn instantiate_left(
        &mut self,
        ctx: Context,
        evar_id: EvarId,
        target: &TypeExpr,
    ) -> Result<Context, ()> {
        let evar_entry = ContextEntry::ExVar(
            evar_id,
            ctx.evar_kind_id(evar_id)
                .unwrap_or_else(|| panic!("ICE: missing evar kind for ?{}", evar_id.0)),
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
            TypeExpr::TyExVar(other_evar) if ctx.evar_before_id(evar_id, *other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: *other_evar,
                    rhs: Self::evar_type(evar_id),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(
                    &ContextEntry::ExVar(
                        *other_evar,
                        ctx.evar_kind_id(*other_evar)
                            .unwrap_or_else(|| panic!("ICE: missing evar kind for ?{}", other_evar.0)),
                    ),
                    &solution,
                ))
            }
            TypeExpr::TyExVar(other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: Self::evar_type(*other_evar),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(&evar_entry, &solution))
            }
            TypeExpr::TyArr(domain, codomain) => {
                let domain_evar = self.fresh_evar_id();
                let codomain_evar = self.fresh_evar_id();
                let arrow_solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: TypeExpr::make_arr(
                        Self::evar_type(domain_evar),
                        Self::evar_type(codomain_evar),
                    ),
                };
                self.record_solution(&arrow_solution);

                left_entries.push(ContextEntry::ExVar(codomain_evar, Kind::Star));
                left_entries.push(ContextEntry::ExVar(domain_evar, Kind::Star));
                left_entries.push(arrow_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);

                let theta = self.instantiate_right(new_ctx, domain, domain_evar)?;
                let codomain_applied = theta.apply(codomain);
                self.instantiate_left(theta, codomain_evar, &codomain_applied)
            }
            TypeExpr::TyApp(func_part, arg_part) => {
                let (func_kind, arg_kind) = self.tyapp_split_kinds(&ctx, evar_id, arg_part)?;

                let func_evar = self.fresh_evar_id();
                let arg_evar = self.fresh_evar_id();

                let app_solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: TypeExpr::make_app(
                        Self::evar_type(func_evar),
                        Self::evar_type(arg_evar),
                    ),
                };

                self.record_solution(&app_solution);

                left_entries.push(ContextEntry::ExVar(arg_evar, arg_kind));
                left_entries.push(ContextEntry::ExVar(func_evar, func_kind));
                left_entries.push(app_solution);

                let new_ctx = Context::from_halves(left_entries, right_entries);

                let theta = self.instantiate_left(new_ctx, func_evar, func_part)?;
                let arg_applied = theta.apply(arg_part);

                self.instantiate_left(theta, arg_evar, &arg_applied)
            }
            TypeExpr::TyForAll(quantifier, kind, body) => {
                let universal_entry = Self::universal_entry(*quantifier, kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());
                let delta = self.instantiate_left(extended_ctx, evar_id, body)?;
                Ok(delta.until(&universal_entry))
            }
            TypeExpr::Qualified(_, body) => self.instantiate_left(ctx, evar_id, body),

            _ => Err(()),
        }
    }
    fn instantiate_right(
        &mut self,
        ctx: Context,
        source: &TypeExpr,
        evar_id: EvarId,
    ) -> Result<Context, ()> {
        let evar_entry = ContextEntry::ExVar(
            evar_id,
            ctx.evar_kind_id(evar_id)
                .unwrap_or_else(|| panic!("ICE: missing evar kind for ?{}", evar_id.0)),
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
            TypeExpr::TyExVar(other_evar) if ctx.evar_before_id(evar_id, *other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: *other_evar,
                    rhs: Self::evar_type(evar_id),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(
                    &ContextEntry::ExVar(
                        *other_evar,
                        ctx.evar_kind_id(*other_evar)
                            .unwrap_or_else(|| panic!("ICE: missing evar kind for ?{}", other_evar.0)),
                    ),
                    &solution,
                ))
            }

            TypeExpr::TyExVar(other_evar) => {
                let solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: Self::evar_type(*other_evar),
                };
                self.record_solution(&solution);
                Ok(ctx.replace(&evar_entry, &solution))
            }
            TypeExpr::TyArr(domain, codomain) => {
                let domain_evar = self.fresh_evar_id();
                let codomain_evar = self.fresh_evar_id();
                let arrow_solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: TypeExpr::make_arr(
                        Self::evar_type(domain_evar),
                        Self::evar_type(codomain_evar),
                    ),
                };
                self.record_solution(&arrow_solution);

                left_entries.push(ContextEntry::ExVar(codomain_evar, Kind::Star));
                left_entries.push(ContextEntry::ExVar(domain_evar, Kind::Star));
                left_entries.push(arrow_solution);
                let new_ctx = Context::from_halves(left_entries, right_entries);
                let theta = self.instantiate_left(new_ctx, domain_evar, domain)?;
                let codomain_applied = theta.apply(codomain);
                self.instantiate_right(theta, &codomain_applied, codomain_evar)
            }
            TypeExpr::TyApp(func_part, arg_part) => {
                let (func_kind, arg_kind) = self.tyapp_split_kinds(&ctx, evar_id, arg_part)?;

                let func_evar = self.fresh_evar_id();
                let arg_evar = self.fresh_evar_id();

                let app_solution = ContextEntry::SolvedEVar {
                    lhs: evar_id,
                    rhs: TypeExpr::make_app(
                        Self::evar_type(func_evar),
                        Self::evar_type(arg_evar),
                    ),
                };

                self.record_solution(&app_solution);

                left_entries.push(ContextEntry::ExVar(arg_evar, arg_kind));
                left_entries.push(ContextEntry::ExVar(func_evar, func_kind));
                left_entries.push(app_solution);

                let new_ctx = Context::from_halves(left_entries, right_entries);

                let theta = self.instantiate_right(new_ctx, func_part, func_evar)?;
                let arg_applied = theta.apply(arg_part);

                self.instantiate_right(theta, &arg_applied, arg_evar)
            }
            TypeExpr::TyForAll(quantifier, kind, body) => {
                let fresh_evar = self.fresh_evar_id();
                let marker = ContextEntry::Marker {
                    var_name: solveig_ast::ast::legacy_evar_name(fresh_evar),
                };
                let extended_ctx = ctx
                    .add_entry(marker.clone())
                    .add_entry(ContextEntry::ExVar(fresh_evar, kind.clone()));

                let instantiated = body.substitute(*quantifier, &Self::evar_type(fresh_evar));
                let delta = self.instantiate_right(extended_ctx, &instantiated, evar_id)?;
                Ok(delta.until(&marker))
            }
            TypeExpr::Qualified(_, body) => self.instantiate_right(ctx, body, evar_id),

            _ => Err(()),
        }
    }
    fn force_arrow(
        &mut self,
        ctx: Context,
        ty: &TypeExpr,
        span: Span,
    ) -> TcResult<(Context, TypeExpr, TypeExpr)> {
        let ty = ctx.apply(ty);

        match &ty {
            TypeExpr::TyArr(domain, codomain) => {
                Ok((ctx, (**domain).clone(), (**codomain).clone()))
            }

            TypeExpr::TyForAll(quantifier, kind, body) => {
                let fresh_evar = self.fresh_evar_id();
                let new_ctx = ctx.add_entry(ContextEntry::ExVar(fresh_evar, kind.clone()));
                let instantiated = body.substitute(*quantifier, &Self::evar_type(fresh_evar));
                self.force_arrow(new_ctx, &instantiated, span)
            }
            TypeExpr::Qualified(_, body) => self.force_arrow(ctx, body, span),

            TypeExpr::TyExVar(evar_id) if ctx.has_unsolved_evar_id(*evar_id) => {
                let evar_entry = ContextEntry::ExVar(
                    *evar_id,
                    ctx.evar_kind_id(*evar_id)
                        .unwrap_or_else(|| panic!("ICE: missing evar kind for ?{}", evar_id.0)),
                );
                let (mut left_entries, _, right_entries) = ctx.split_at(&evar_entry);

                let domain_evar = self.fresh_evar_id();
                let codomain_evar = self.fresh_evar_id();
                let arrow_solution = ContextEntry::SolvedEVar {
                    lhs: *evar_id,
                    rhs: TypeExpr::make_arr(
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
                actual: ty.pretty(),
            }),
        }
    }
}
