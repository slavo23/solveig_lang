impl TypeChecker {
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
                        ty: self.pretty_type(annotation),
                        target: format!("parameter `{}`", param),
                    });
                }
                Ok((ctx, annotation.clone()))
            }
            None => {
                let evar_id = self.fresh_evar_id();
                let new_ctx = ctx.add_entry(ContextEntry::ExVar(evar_id, Kind::Star));
                Ok((new_ctx, Self::evar_type(evar_id)))
            }
        }
    }
    fn int_type(&self) -> TypeExpr {
        TypeExpr::make_con_id(PRIM_TYS.int)
    }
    fn float_type(&self) -> TypeExpr {
        TypeExpr::make_con_id(PRIM_TYS.float)
    }
    fn char_type(&self) -> TypeExpr {
        TypeExpr::make_con_id(PRIM_TYS.char)
    }
    fn string_type(&self) -> TypeExpr {
        TypeExpr::make_con_id(PRIM_TYS.string)
    }
    fn literal_type(&self, lit: &Literal) -> TypeExpr {
        match lit {
            Literal::Int(_) => self.int_type(),
            Literal::Float(_) => self.float_type(),
            Literal::Char(_) => self.char_type(),
            Literal::Str(_) => self.string_type(),
        }
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
                let display_name = self.names.var_display_name(name);

                if matches!(name, VarRef::Local(_))
                    && let Some(var_type) = self.lookup_local_term_var(&ctx, &display_name)
                {
                    let var_type = ctx.apply(&var_type);
                    let elaborated = elab_node(TermExpr::Var(name.clone()), span, var_type.clone());
                    return Ok((ctx, elaborated, var_type));
                }

                if let VarRef::Global(def_id) = name
                    && let Some(scheme) = self.func_schemes.get(def_id).cloned()
                {
                    let (ctx, preds, instantiated_ty) = self.instantiate_scheme(ctx, &scheme);
                    let evidence_args = self.add_wanteds(preds, span.clone());

                    let instantiated_ty = ctx.apply(&instantiated_ty);
                    let elaborated =
                        elab_node(TermExpr::Var(name.clone()), span.clone(), instantiated_ty.clone());
                    let elaborated =
                        wrap_ev_apps(elaborated, evidence_args, span, instantiated_ty.clone());
                    return Ok((ctx, elaborated, instantiated_ty));
                }
                if let VarRef::Global(def_id) = name
                    && let Some(method_scheme) = self.lookup_class_method_scheme(*def_id)
                {
                    let (ctx, preds, instantiated_ty) =
                        self.instantiate_scheme(ctx, &method_scheme);

                    let evidence_args = self.add_wanteds(preds, span.clone());

                    let instantiated_ty = ctx.apply(&instantiated_ty);
                    let elaborated =
                        elab_node(TermExpr::Var(name.clone()), span.clone(), instantiated_ty.clone());
                    let elaborated =
                        wrap_ev_apps(elaborated, evidence_args, span, instantiated_ty.clone());
                    return Ok((ctx, elaborated, instantiated_ty));
                }

                Err(TypeError::UnboundVariable {
                    span: self.get_file_span(span.clone()),
                    name: display_name,
                })
            }
            TermExpr::Con(id) => {
                let constructor_type =
                    self.constructor_types.get(id).cloned().ok_or_else(|| {
                        TypeError::UnknownConstructor {
                            span: self.get_file_span(span.clone()),
                            name: self.display_ctor_name(*id),
                        }
                    })?;
                let elaborated = elab_node(TermExpr::Con(*id), span, constructor_type.clone());
                Ok((ctx, elaborated, constructor_type))
            }
            TermExpr::Abs(param, maybe_annotation, body) => {
                let (ctx, param_type) =
                    self.infer_lambda_param_type(ctx, param, maybe_annotation, span.clone())?;

                let result_evar = self.fresh_evar_id();
                let typed_var_entry = ContextEntry::TypedVar {
                    var_name: param.clone(),
                    ty: param_type.clone(),
                };

                let extended_ctx = ctx
                    .add_entry(ContextEntry::ExVar(result_evar, Kind::Star))
                    .add_entry(typed_var_entry.clone());

                let body_expected_type = Self::evar_type(result_evar);
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
                let param_id = source_tyvar_id(param);
                let universal_entry = Self::universal_entry(param_id, kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());

                let (delta, elaborated_body, body_type) =
                    self.solve_inference(extended_ctx, body)?;

                let forall_type = TypeExpr::make_forall_id(param_id, kind.clone(), body_type);
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
                    TypeExpr::TyForAll(ref binders, ref preds, ref body)
                        if let Some((quantifier, _)) = binders.first() =>
                    {
                        let subst = HashMap::from([(*quantifier, ty_arg.clone())]);
                        let result_type = TypeExpr::make_forall_group(
                            binders[1..].to_vec(),
                            preds
                                .iter()
                                .map(|pred| Self::substitute_predicate(pred, &subst))
                                .collect(),
                            body.substitute(*quantifier, ty_arg),
                        );
                        let elaborated = elab_node(
                            TermExpr::TyApp(Box::new(elaborated_inner), ty_arg.clone()),
                            span,
                            result_type.clone(),
                        );
                        Ok((delta, elaborated, result_type))
                    }
                    other => Err(TypeError::NonPolymorphicTypeApplication {
                        span: self.get_file_span(span),
                        actual: self.pretty_type(&other),
                        ty_arg: self.pretty_type(ty_arg),
                    }),
                }
            }
            TermExpr::Case(scrutinee, arms) => {
                self.solve_case_inference(ctx, scrutinee, arms, span)
            }

            TermExpr::IfThenElse(cond, then_branch, else_branch) => {
                let bool_type = self.bool_type();
                let (gamma, elaborated_cond) = self.solve_checking(ctx, cond, &bool_type)?;

                let result_evar = self.fresh_evar_id();
                let result_type = Self::evar_type(result_evar);
                let theta = gamma.add_entry(ContextEntry::ExVar(result_evar, Kind::Star));

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
                let ty = self.literal_type(lit);

                let elaborated = elab_node(TermExpr::Lit(lit.clone()), span, ty.clone());
                Ok((ctx, elaborated, ty))
            }
            TermExpr::Hole => {
                let evar_id = self.fresh_evar_id();
                let evar_type = Self::evar_type(evar_id);
                let extended_ctx = ctx.add_entry(ContextEntry::ExVar(evar_id, Kind::Star));
                self.report_hole(&extended_ctx, span.clone(), &evar_type);
                let elaborated = elab_node(TermExpr::Hole, span, evar_type.clone());
                Ok((extended_ctx, elaborated, evar_type))
            }
            TermExpr::EvAbs(_, _, _) | TermExpr::EvApp(_, _) => {
                panic!("ICE: evidence terms are introduced after source expression inference")
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
            TypeExpr::TyForAll(binders, preds, body) if !binders.is_empty() => {
                let (quantifier, kind) = &binders[0];
                let fresh_evar = self.fresh_evar_id();
                let evar_entry = ContextEntry::ExVar(fresh_evar, kind.clone());
                let extended_ctx = ctx.add_entry(evar_entry);

                let ty_arg = Self::evar_type(fresh_evar);
                let subst = HashMap::from([(*quantifier, ty_arg.clone())]);
                let instantiated_type = TypeExpr::make_forall_group(
                    binders[1..].to_vec(),
                    preds
                        .iter()
                        .map(|pred| Self::substitute_predicate(pred, &subst))
                        .collect(),
                    body.substitute(*quantifier, &ty_arg),
                );

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
            TypeExpr::TyForAll(_, preds, body) => {
                let evidence_args = self.add_wanteds(preds.clone(), span.clone());
                let body_type = (**body).clone();
                let wrapped_function = wrap_ev_apps(
                    elaborated_function.clone(),
                    evidence_args,
                    span.clone(),
                    body_type.clone(),
                );
                self.solve_app_inference(ctx, &body_type, argument, &wrapped_function, span, arg_span)
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

                let (delta, elaborated_argument) =
                    self.solve_checking(new_ctx, argument, &Self::evar_type(domain_evar))?;

                let result_type = delta.apply(&Self::evar_type(codomain_evar));
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
                actual: self.pretty_type(other),
            }),
        }
    }
}
