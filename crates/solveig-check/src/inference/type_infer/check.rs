impl TypeChecker {
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
                TypeExpr::TyForAll(binders, preds, body_type),
            ) if let Some((quantifier, quant_kind)) = binders.first() => {
                let param_id = legacy_tyvar_id(param);
                let ty_arg = TypeExpr::TyVar(param_id);
                let subst = HashMap::from([(*quantifier, ty_arg.clone())]);
                let substituted_body_type = TypeExpr::make_forall_group(
                    binders[1..].to_vec(),
                    preds
                        .iter()
                        .map(|pred| Self::substitute_predicate(pred, &subst))
                        .collect(),
                    body_type.substitute(*quantifier, &ty_arg),
                );
                let universal_entry = Self::universal_entry(param_id, quant_kind.clone());
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

            (_, TypeExpr::TyForAll(binders, preds, body_type)) if !binders.is_empty() => {
                let (quantifier, kind) = &binders[0];
                let universal_entry = Self::universal_entry(*quantifier, kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());
                let body_type = TypeExpr::make_forall_group(
                    binders[1..].to_vec(),
                    preds.clone(),
                    (**body_type).clone(),
                );

                let (delta, elaborated_body) =
                    self.solve_checking(extended_ctx, expr, &body_type)?;

                let result_type = expected_type.clone();
                let elaborated = elab_node(
                    TermExpr::TyAbs(
                        tyvar_display_name(*quantifier),
                        kind.clone(),
                        Box::new(elaborated_body),
                    ),
                    span,
                    result_type,
                );
                Ok((delta.until(&universal_entry), elaborated))
            }
            (_, TypeExpr::TyForAll(_, preds, body)) => {
                let preds = preds.clone();
                let (givens, result) =
                    self.with_givens(preds, |checker| checker.solve_checking(ctx, expr, body));
                let (delta, elaborated_body) = result?;
                let result_type = delta.apply(expected_type);
                let elaborated = wrap_ev_abs(elaborated_body, givens, span, result_type);
                Ok((delta, elaborated))
            }

            _ => {
                let (inferred_ctx, elaborated_expr, inferred_type) =
                    self.solve_inference(ctx, expr)?;
                let inferred_applied = inferred_ctx.apply(&inferred_type);
                let expected_applied = inferred_ctx.apply(expected_type);
                let wanted_start = self.wanteds.len();
                let delta = self.solve_subtype(
                    inferred_ctx,
                    &inferred_applied,
                    &expected_applied,
                    self.get_file_span(span),
                )?;
                let evidence_args = self.wanteds[wanted_start..]
                    .iter()
                    .map(|wanted| wanted.ev)
                    .collect::<Vec<_>>();
                let result_type = delta.apply(&expected_applied);
                let elaborated_expr =
                    wrap_ev_apps(elaborated_expr, evidence_args, expr.anno.clone(), result_type);
                Ok((delta, elaborated_expr))
            }
        }
    }
}

fn orphan_instance_warning(
    inst: &InstanceDefinition<Span>,
    env: &TypeCheckEnv,
    file: FileId,
) -> Option<TypeError> {
    if inst.class_module == Some(inst.def_module) || inst.type_module == Some(inst.def_module) {
        return None;
    }

    let class_name = env.display_class_name(inst.head.class_id);
    let instance_type = inst
        .head
        .args
        .first()
        .map(|ty| ty.pretty_with_tables(env.pretty_tables()))
        .unwrap_or_else(|| "_".to_string());

    Some(TypeError::OrphanInstance {
        span: file_span(file, inst.span.clone()),
        class_name,
        instance_type,
    })
}

pub fn check_and_elab_module(
    module: &ResolvedModule<Span>,
    env: &TypeCheckEnv,
    file: FileId,
) -> Result<TCOutput, Vec<TypeError>> {
    let mut errors = Vec::new();

    let mut checked_defs = Vec::new();
    let mut checked_instance_defs = Vec::new();

    let mut available_func_schemes = env.func_schemes.clone();
    let mut checked_func_schemes = module
        .builtin_defs
        .iter()
        .map(|builtin| (builtin.id, builtin.scheme.clone()))
        .collect::<HashMap<DefId, Scheme>>();

    let mut all_holes = Vec::new();
    let mut all_warnings = Vec::new();

    {
        let checker = TypeChecker::with_env(env, file);
        let overlap_errors = checker.check_instance_overlaps();
        if !overlap_errors.is_empty() {
            return Err(overlap_errors);
        }
    }

    for class_def in &module.class_defs {
        for method in &class_def.methods {
            let Some(default_body) = &method.default_body else {
                continue;
            };

            let expected_scheme =
                TypeChecker::class_method_scheme_for_term_lookup(class_def, method);
            let expected_type = TypeChecker::scheme_to_type(&expected_scheme);

            let mut checker = TypeChecker::with_env(env, file);
            checker.func_schemes = available_func_schemes.clone();

            match checker.solve_checking(Context::new(), default_body, &expected_type) {
                Ok((_final_ctx, elaborated_default)) => {
                    let unresolved = checker.unresolved_wanteds();

                    if unresolved.is_empty() {
                        let _ = checker.zonk_expr(&elaborated_default);
                    } else {
                        let cause = checker.wanteds_error(unresolved);

                        errors.push(TypeError::InstanceMethodTypeMismatch {
                            span: file_span(file, default_body.anno.clone()),
                            method: method.short_name.clone(),
                            class_name: class_def.name.clone(),
                            expected: checker.pretty_scheme(&expected_scheme),
                            cause: Box::new(cause),
                        });
                    }
                }

                Err(cause) => {
                    errors.push(TypeError::InstanceMethodTypeMismatch {
                        span: file_span(file, default_body.anno.clone()),
                        method: method.short_name.clone(),
                        class_name: class_def.name.clone(),
                        expected: checker.pretty_scheme(&expected_scheme),
                        cause: Box::new(cause),
                    });
                }
            }

            checker.finalize_holes();
            all_holes.extend(checker.holes);
            all_warnings.extend(checker.warnings);
        }
    }

    for inst in &module.instance_defs {
        if let Some(warning) = orphan_instance_warning(inst, env, file) {
            all_warnings.push(warning);
        }
    }

    for inst in &module.instance_defs {
        let env_pretty_tables = env.pretty_tables();
        let inst_class_name = env.display_class_name(inst.head.class_id);
        let class_def = env.class_defs.get(&inst.head.class_id);

        let class_def = match class_def {
            Some(class_def) => class_def,
            None => {
                errors.push(TypeError::UnboundVariable {
                    span: file_span(file, 0..0),
                    name: format!("unknown class `{}` in instance", inst_class_name),
                });
                continue;
            }
        };

        let superclass_obligations =
            TypeChecker::specialize_superclasses_for_instance(class_def, &inst.head);

        let mut checker = TypeChecker::with_env(env, file);

        // While validating `instance C t`, do not use that same instance as evidence
        // for its own superclass obligations. This avoids accepting recursive proofs
        // such as constructing `Eq Int` through the `Ord Int` dictionary currently
        // being checked.
        let superclass_instance_defs = env
            .instance_defs
            .iter()
            .filter(|candidate| candidate.id != inst.id)
            .cloned()
            .collect::<Vec<_>>();
        checker.instance_table = InstanceTable::from_definitions(superclass_instance_defs);

        let (context_givens, superclass_results) =
            checker.solve_predicates_with_givens(&superclass_obligations, &inst.context);
        let context_evidence = context_givens
            .iter()
            .map(|given| given.ev)
            .collect::<Vec<_>>();
        let mut superclass_evidence = Vec::new();
        let mut missing_superclasses = Vec::new();

        for (pred, result) in superclass_obligations.iter().zip(superclass_results) {
            match result {
                Ok(ev) => superclass_evidence.push(checker.zonk_ev(&ev)),
                Err(_) => missing_superclasses.push(format!(
                    "{} required by superclass of {}",
                    pred.pretty_with_tables(env_pretty_tables),
                    inst.head.pretty_with_tables(env_pretty_tables)
                )),
            }
        }

        if !missing_superclasses.is_empty() {
            let span = inst
                .methods
                .first()
                .map(|method| method.body.anno.clone())
                .unwrap_or(0..0);

            errors.push(TypeError::UnsatisfiedConstraints {
                span: file_span(file, span),
                predicates: missing_superclasses,
            });
        }

        let mut checked_methods = Vec::new();

        for inst_method in &inst.methods {
            if !class_def.methods.iter().any(|method| {
                method.name == inst_method.name || method.short_name == inst_method.name
            }) {
                errors.push(TypeError::UnboundVariable {
                    span: file_span(file, inst_method.body.anno.clone()),
                    name: format!(
                        "instance method `{}` not found in class `{}`",
                        inst_method.name, inst_class_name
                    ),
                });
            }
        }

        let instance_quantifiers = TypeChecker::instance_quantifiers(inst);

        for class_method in &class_def.methods {
            let explicit_method = inst.methods.iter().find(|method| {
                class_method.name == method.name || class_method.short_name == method.name
            });
            let (method_name, method_body) = if let Some(inst_method) = explicit_method {
                (inst_method.name.clone(), &inst_method.body)
            } else if let Some(default_body) = &class_method.default_body {
                (class_method.short_name.clone(), default_body)
            } else {
                errors.push(TypeError::MissingInstanceMethod {
                    span: file_span(file, inst.span.clone()),
                    method: class_method.short_name.clone(),
                    class_name: inst_class_name.clone(),
                });
                continue;
            };

            let expected_scheme = TypeChecker::specialize_class_method_for_instance(
                class_def,
                &class_method.scheme,
                &inst.head,
                &inst.context,
                &instance_quantifiers,
            );

            let expected_type = TypeChecker::scheme_to_type(&expected_scheme);

            let mut checker = TypeChecker::with_env(env, file);
            checker.func_schemes = available_func_schemes.clone();

            match checker.solve_checking(Context::new(), method_body, &expected_type) {
                Ok((_final_ctx, elaborated_method)) => {
                    let unresolved = checker.unresolved_wanteds();

                    if unresolved.is_empty() {
                        let zonked_method = checker.zonk_expr(&elaborated_method);

                        checked_methods.push(InstanceMethodDef {
                            name: method_name.clone(),
                            body: zonked_method,
                        });
                    } else {
                        let cause = checker.wanteds_error(unresolved);

                        errors.push(TypeError::InstanceMethodTypeMismatch {
                            span: file_span(file, method_body.anno.clone()),
                            method: method_name.clone(),
                            class_name: inst_class_name.clone(),
                            expected: checker.pretty_scheme(&expected_scheme),
                            cause: Box::new(cause),
                        });
                    }
                }

                Err(cause) => {
                    errors.push(TypeError::InstanceMethodTypeMismatch {
                        span: file_span(file, method_body.anno.clone()),
                        method: method_name.clone(),
                        class_name: inst_class_name.clone(),
                        expected: checker.pretty_scheme(&expected_scheme),
                        cause: Box::new(cause),
                    });
                }
            }

            checker.finalize_holes();
            all_holes.extend(checker.holes);
            all_warnings.extend(checker.warnings);
        }

        checked_instance_defs.push(InstanceDefinition {
            id: inst.id,
            span: inst.span.clone(),
            def_module: inst.def_module,
            class_module: inst.class_module,
            type_module: inst.type_module,
            context: inst.context.clone(),
            context_evidence,
            head: inst.head.clone(),
            superclass_evidence,
            methods: checked_methods,
        });
    }

    for func_def in &module.func_defs {
        let mut checker = TypeChecker::with_env(env, file);
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
                                id: func_def.id,
                                name: func_def.name.clone(),
                                short_name: func_def.short_name.clone(),
                                signature: Some(final_scheme.clone()),
                                body: zonked_expr,
                            });

                            available_func_schemes.insert(func_def.id, final_scheme.clone());
                            checked_func_schemes.insert(func_def.id, final_scheme);
                        }

                        let _ = final_ctx;
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
                            id: func_def.id,
                            name: func_def.name.clone(),
                            short_name: func_def.short_name.clone(),
                            signature: Some(final_scheme.clone()),
                            body: zonked_expr,
                        });

                        available_func_schemes.insert(func_def.id, final_scheme.clone());
                        checked_func_schemes.insert(func_def.id, final_scheme);
                    }
                }

                Err(err) => {
                    errors.push(err);
                }
            },
        }

        checker.finalize_holes();
        all_holes.extend(checker.holes);
        all_warnings.extend(checker.warnings);
    }

    if errors.is_empty() {
        Ok(TCOutput {
            module: ResolvedModule {
                data_defs: module.data_defs.clone(),
                class_defs: module.class_defs.clone(),
                instance_defs: checked_instance_defs,
                builtin_defs: module.builtin_defs.clone(),
                tycon_table: module.tycon_table.clone(),
                ctor_table: module.ctor_table.clone(),
                class_table: module.class_table.clone(),
                tycon_kinds: module.tycon_kinds.clone(),
                constructor_types: module.constructor_types.clone(),
                func_schemes: checked_func_schemes,
                func_defs: checked_defs,
            },
            holes: all_holes,
            diagnostics: all_warnings,
        })
    } else {
        errors.extend(all_warnings);
        Err(errors)
    }
}

pub fn check_and_elab_expr_in_checked_module(
    module: &CheckedModule,
    file: FileId,
    expr: &TermExprNode<Span>,
    expected_type: &TypeExpr,
) -> Result<Option<TermExprNode<TcMetadata, Span>>, TypeError> {
    let env = TypeCheckEnv::from_checked_module(module);
    let mut checker = TypeChecker::with_env(&env, file);
    let (_ctx, elaborated) = checker.solve_checking(Context::new(), expr, expected_type)?;
    let unresolved = checker.unresolved_wanteds();

    if unresolved.is_empty() {
        Ok(Some(checker.zonk_expr(&elaborated)))
    } else {
        Ok(None)
    }
}
