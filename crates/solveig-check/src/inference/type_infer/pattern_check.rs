impl TypeChecker {
    fn bool_type(&self) -> TypeExpr {
        TypeExpr::make_con("prelude.builtins.Bool".to_string())
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
                    origin,
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
        span: Span,
    ) -> TcResult<(Context, Vec<TypeExpr>, TypeExpr)> {
        let mut theta = ctx;
        let mut current_type = constructor_type.clone();
        let mut field_types = Vec::with_capacity(arity);

        for _ in 0..arity {
            let (new_ctx, domain, codomain) = self.force_arrow(theta, &current_type, span.clone())?;
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

        let result_evar = self.fresh_evar_id();
        let result_type = Self::evar_type(result_evar);
        let mut theta = gamma.add_entry(ContextEntry::ExVar(result_evar, Kind::Star));

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

            if !omega.has_unsolved_evar_id(result_evar) && !omega.has_solved_evar_id(result_evar) {
                return Err(TypeError::MissingCaseResultExistential {
                    span: self.get_file_span(arm.node.expr.anno.clone()),
                    evar: solveig_ast::ast::legacy_evar_name(result_evar),
                    arm_index,
                });
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

        let scrutinee_type_for_exhaustiveness = self.zonk_type(&theta.apply(&scrutinee_type));

        exhaustiveness::check_case(
            self.file,
            &self.constructor_types,
            &scrutinee_type_for_exhaustiveness,
            arms,
            span.clone(),
        )?;

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
            Pattern::Lit(lit) => {
                let literal_type = match lit {
                    Literal::Int(_) => self.int_type(),
                };

                let theta = self.solve_subtype(
                    ctx.clone(),
                    &literal_type,
                    &ctx.apply(&scrutinee_type),
                    self.get_file_span(pattern.anno.clone()),
                )?;

                Ok((theta, vec![]))
            }
            Pattern::Con(constructor_id, sub_patterns) => {
                let constructor_name = ctor_display_name(*constructor_id);
                let constructor_type = self
                    .constructor_types
                    .get(&constructor_name)
                    .cloned()
                    .ok_or_else(|| TypeError::UnknownPatternConstructor {
                        span: self.get_file_span(pattern.anno.clone()),
                        name: constructor_name,
                    })?;

                let (mut theta, field_types, constructor_result_type) = self
                    .instantiate_constructor_pattern(
                        ctx,
                        &constructor_type,
                        sub_patterns.len(),
                        pattern.anno.clone(),
                    )?;

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
}
