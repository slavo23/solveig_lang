impl TypeChecker<'_> {
    pub fn solve_checking(
        &mut self,
        ctx: Context,
        expr: &TermExprNode<Span>,
        expected_type: &Ty,
    ) -> TcResult<(Context, TermExprNode<TcMetadata, Span>)> {
        let expected_type = &ctx.apply(expected_type);
        let span = expr.anno.clone();

        match (&expr.node, expected_type) {
            (TermExpr::Unit, Ty::TyUnit) => {
                let elaborated = elab_node(TermExpr::Unit, span, Ty::TyUnit);
                Ok((ctx, elaborated))
            }

            (
                TermExpr::TyAbs(param, _kind, body),
                Ty::TyForAll(binders, preds, body_type),
            ) if let Some((quantifier, quant_kind)) = binders.first() => {
                let param_id = *quantifier;
                let ty_arg = Ty::TyVar(param_id);
                let subst = FxHashMap::from_iter([(*quantifier, ty_arg.clone())]);
                let substituted_body_type = Ty::make_forall_group(
                    binders[1..].to_vec(),
                    preds
                        .iter()
                        .map(|pred| Self::substitute_predicate(pred, &subst))
                        .collect(),
                    body_type.substitute(*quantifier, &ty_arg),
                );
                let universal_entry = self.universal_entry(param_id, quant_kind.clone());
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

            (TermExpr::Abs(param, maybe_annotation, body), Ty::TyArr(domain, codomain)) => {
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

            (_, Ty::TyForAll(binders, preds, body_type)) if !binders.is_empty() => {
                let (quantifier, kind) = &binders[0];
                let universal_entry = self.universal_entry(*quantifier, kind.clone());
                let extended_ctx = ctx.add_entry(universal_entry.clone());
                let body_type = Ty::make_forall_group(
                    binders[1..].to_vec(),
                    preds.clone(),
                    (**body_type).clone(),
                );

                let (delta, elaborated_body) =
                    self.solve_checking(extended_ctx, expr, &body_type)?;

                let result_type = expected_type.clone();
                let elaborated = elab_node(
                    TermExpr::TyAbs(
                        self.tyvar_display_name(*quantifier),
                        kind.clone(),
                        Box::new(elaborated_body),
                    ),
                    span,
                    result_type,
                );
                Ok((delta.until(&universal_entry), elaborated))
            }
            (_, Ty::TyForAll(_, preds, body)) => {
                let preds = preds.clone();
                let (givens, result) =
                    self.with_givens(preds, |checker| checker.solve_checking(ctx, expr, body));
                let (delta, elaborated_body) = result?;
                let result_type = delta.apply(expected_type);
                let elaborated = wrap_ev_abs(elaborated_body, givens, span, &result_type);
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
                    wrap_ev_apps(elaborated_expr, evidence_args, expr.anno.clone(), &result_type);
                Ok((delta, elaborated_expr))
            }
        }
    }
}

fn orphan_instance_warning(
    inst: &InstanceDefinition<Span>,
    env: &TypeCheckEnv,
    names: &Names,
    file: FileId,
) -> Option<TypeError> {
    if inst.class_module == Some(inst.def_module) || inst.type_module == Some(inst.def_module) {
        return None;
    }

    let class_name = env.display_class_name(names, inst.head.class_id);
    let instance_type = inst
        .head
        .args
        .first()
        .map_or_else(|| "_".to_string(), |ty| ty.pretty_with_tables(env.pretty_tables(names)));

    Some(TypeError::OrphanInstance {
        span: file_span(file, inst.span.clone()),
        class_name,
        instance_type,
    })
}

struct FunctionDependencyGroups {
    groups: Vec<Vec<usize>>,
    adjacency: Vec<Vec<usize>>,
}

enum FunctionSignatureSource {
    Explicit(Scheme),
    Inferred(Ty),
}

fn substitute_generalized_type(ty: &Ty, subst: &FxHashMap<EvarId, Ty>) -> Ty {
    subst
        .iter()
        .fold(ty.clone(), |acc, (evar, replacement)| {
            acc.substitute_evar(*evar, replacement)
        })
}

fn substitute_generalized_predicate(
    pred: &Predicate,
    subst: &FxHashMap<EvarId, Ty>,
) -> Predicate {
    Predicate {
        class_id: pred.class_id,
        args: pred
            .args
            .iter()
            .map(|arg| substitute_generalized_type(arg, subst))
            .collect(),
    }
}

fn substitute_generalized_ev(ev: Ev, subst: &FxHashMap<EvarId, Ty>) -> Ev {
    match ev {
        Ev::Var(_) | Ev::Placeholder(_) => ev,
        Ev::Inst(id, ty_args, ev_args) => Ev::Inst(
            id,
            ty_args
                .into_iter()
                .map(|ty| substitute_generalized_type(&ty, subst))
                .collect(),
            ev_args
                .into_iter()
                .map(|ev| substitute_generalized_ev(ev, subst))
                .collect(),
        ),
        Ev::Super(inner, class_id, index) => Ev::Super(
            Box::new(substitute_generalized_ev(*inner, subst)),
            class_id,
            index,
        ),
    }
}

fn substitute_generalized_expr(
    expr: TermExprNode<TcMetadata, Span>,
    subst: &FxHashMap<EvarId, Ty>,
) -> TermExprNode<TcMetadata, Span> {
    let span = expr.anno.span;
    let ty = substitute_generalized_type(&expr.anno.inferred_type, subst);
    let node = match expr.node {
        TermExpr::Unit => TermExpr::Unit,
        TermExpr::Var(var) => TermExpr::Var(var),
        TermExpr::Lit(lit) => TermExpr::Lit(lit),
        TermExpr::Hole => TermExpr::Hole,
        TermExpr::App(func, arg) => TermExpr::App(
            Box::new(substitute_generalized_expr(*func, subst)),
            Box::new(substitute_generalized_expr(*arg, subst)),
        ),
        TermExpr::Abs(param, annotation, body) => TermExpr::Abs(
            param,
            annotation.map(|ty| substitute_generalized_type(&ty, subst)),
            Box::new(substitute_generalized_expr(*body, subst)),
        ),
        TermExpr::TyAbs(param, kind, body) => {
            TermExpr::TyAbs(param, kind, Box::new(substitute_generalized_expr(*body, subst)))
        }
        TermExpr::TyApp(inner, ty) => TermExpr::TyApp(
            Box::new(substitute_generalized_expr(*inner, subst)),
            substitute_generalized_type(&ty, subst),
        ),
        TermExpr::EvAbs(ev, pred, body) => TermExpr::EvAbs(
            ev,
            substitute_generalized_predicate(&pred, subst),
            Box::new(substitute_generalized_expr(*body, subst)),
        ),
        TermExpr::EvApp(inner, ev) => TermExpr::EvApp(
            Box::new(substitute_generalized_expr(*inner, subst)),
            substitute_generalized_ev(ev, subst),
        ),
        TermExpr::Anno(inner, ty) => TermExpr::Anno(
            Box::new(substitute_generalized_expr(*inner, subst)),
            substitute_generalized_type(&ty, subst),
        ),
        TermExpr::Con(id) => TermExpr::Con(id),
        TermExpr::Case(scrutinee, arms) => TermExpr::Case(
            Box::new(substitute_generalized_expr(*scrutinee, subst)),
            arms.into_iter()
                .map(|arm| {
                    CaseArmNode::new(
                        CaseArm {
                            pattern: arm.node.pattern,
                            expr: substitute_generalized_expr(arm.node.expr, subst),
                        },
                        arm.anno,
                    )
                })
                .collect(),
        ),
        TermExpr::IfThenElse(cond, then_branch, else_branch) => TermExpr::IfThenElse(
            Box::new(substitute_generalized_expr(*cond, subst)),
            Box::new(substitute_generalized_expr(*then_branch, subst)),
            Box::new(substitute_generalized_expr(*else_branch, subst)),
        ),
        TermExpr::Let(name, binder, body) => TermExpr::Let(
            name,
            Box::new(substitute_generalized_expr(*binder, subst)),
            Box::new(substitute_generalized_expr(*body, subst)),
        ),
    };

    elab_node(node, span, ty)
}

fn wrap_inferred_quantifiers(
    names: &Names,
    name_scratch: &NameScratch,
    body: TermExprNode<TcMetadata, Span>,
    scheme: &Scheme,
    retained: Vec<GivenEvidence>,
    span: Span,
) -> TermExprNode<TcMetadata, Span> {
    let qualified_type =
        Ty::make_forall_group(Vec::new(), scheme.predicates.clone(), scheme.ty.clone());
    let mut body = wrap_ev_abs(body, retained, span.clone(), &qualified_type);

    for index in (0..scheme.quantifiers.len()).rev() {
        let (param, kind) = &scheme.quantifiers[index];
        let result_type = Ty::make_forall_group(
            scheme.quantifiers[index..].to_vec(),
            scheme.predicates.clone(),
            scheme.ty.clone(),
        );
        body = elab_node(
            TermExpr::TyAbs(
                name_scratch.tyvar_display_name(names, *param),
                kind.clone(),
                Box::new(body),
            ),
            span.clone(),
            result_type,
        );
    }

    body
}

fn collect_function_deps<A, P>(
    expr: &TermExprNode<A, P>,
    local_indices: &FxHashMap<DefId, usize>,
    out: &mut FxHashSet<usize>,
) {
    match &expr.node {
        TermExpr::Var(VarRef::Global(def_id)) => {
            if let Some(index) = local_indices.get(def_id) {
                out.insert(*index);
            }
        }
        TermExpr::App(func, arg) => {
            collect_function_deps(func, local_indices, out);
            collect_function_deps(arg, local_indices, out);
        }
        TermExpr::Abs(_, _, body)
        | TermExpr::TyAbs(_, _, body)
        | TermExpr::TyApp(body, _)
        | TermExpr::EvAbs(_, _, body)
        | TermExpr::EvApp(body, _)
        | TermExpr::Anno(body, _) => {
            collect_function_deps(body, local_indices, out);
        }
        TermExpr::Case(scrutinee, arms) => {
            collect_function_deps(scrutinee, local_indices, out);
            for arm in arms {
                collect_function_deps(&arm.node.expr, local_indices, out);
            }
        }
        TermExpr::IfThenElse(cond, then_branch, else_branch) => {
            collect_function_deps(cond, local_indices, out);
            collect_function_deps(then_branch, local_indices, out);
            collect_function_deps(else_branch, local_indices, out);
        }
        TermExpr::Let(_, binder, body) => {
            collect_function_deps(binder, local_indices, out);
            collect_function_deps(body, local_indices, out);
        }
        TermExpr::Unit
        | TermExpr::Lit(_)
        | TermExpr::Hole
        | TermExpr::Con(_)
        | TermExpr::Var(VarRef::Local(_)) => {}
    }
}

fn function_dependency_groups(func_defs: &[TermDefinition<Span>]) -> FunctionDependencyGroups {
    let local_indices = func_defs
        .iter()
        .enumerate()
        .map(|(index, def)| (def.id, index))
        .collect::<FxHashMap<_, _>>();
    let mut adjacency = vec![Vec::new(); func_defs.len()];

    for (index, def) in func_defs.iter().enumerate() {
        let mut deps = FxHashSet::default();
        collect_function_deps(&def.body, &local_indices, &mut deps);

        for candidate in 0..func_defs.len() {
            if deps.contains(&candidate) {
                adjacency[index].push(candidate);
            }
        }
    }

    FunctionDependencyGroups {
        groups: crate::dependency_graph::dependency_ordered_sccs(&adjacency),
        adjacency,
    }
}

fn function_group_needs_placeholders(group: &[usize], adjacency: &[Vec<usize>]) -> bool {
    group.len() > 1 || group.iter().any(|index| adjacency[*index].contains(index))
}

fn restore_func_scheme_placeholders(
    func_schemes: &mut FxHashMap<DefId, Scheme>,
    placeholders: Vec<(DefId, Option<Scheme>)>,
) {
    for (id, previous) in placeholders {
        if let Some(scheme) = previous {
            func_schemes.insert(id, scheme);
        } else {
            func_schemes.remove(&id);
        }
    }
}

pub fn check_and_elab_module(
    module: &ResolvedModule<Span>,
    env: &TypeCheckEnv,
    names: &Names,
    file: FileId,
) -> Result<TCOutput, Vec<TypeError>> {
    let mut errors = Vec::new();
    let tables = ModuleTables::from_env(env);
    let mut delta = CheckDelta::from_env(env, names);

    let mut checked_instance_defs = Vec::new();

    let mut checked_func_schemes = module
        .builtin_defs
        .iter()
        .map(|builtin| (builtin.id, builtin.scheme.clone()))
        .collect::<FxHashMap<DefId, Scheme>>();

    let mut all_holes = Vec::new();
    let mut all_warnings = Vec::new();

    {
        let checker = TypeChecker::with_delta(tables, names, file, delta);
        let overlap_errors = checker.check_instance_overlaps();
        let (next_delta, _, _) = checker.finish();
        delta = next_delta;
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

            let mut checker = TypeChecker::with_delta(tables, names, file, delta);

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
            let (next_delta, holes, warnings) = checker.finish();
            delta = next_delta;
            all_holes.extend(holes);
            all_warnings.extend(warnings);
        }
    }

    for inst in &module.instance_defs {
        if let Some(warning) = orphan_instance_warning(inst, env, names, file) {
            all_warnings.push(warning);
        }
    }

    for inst in &module.instance_defs {
        let env_pretty_tables = env.pretty_tables(names);
        let inst_class_name = env.display_class_name(names, inst.head.class_id);
        let class_def = env.class_defs.get(&inst.head.class_id);

        let class_def = match class_def {
            Some(class_def) => class_def,
            None => {
                errors.push(TypeError::UnboundVariable {
                    span: file_span(file, 0..0),
                    name: format!("unknown class `{inst_class_name}` in instance"),
                });
                continue;
            }
        };

        let superclass_obligations =
            TypeChecker::specialize_superclasses_for_instance(class_def, &inst.head);

        let mut checker = TypeChecker::with_delta(tables, names, file, delta);

        // While validating `instance C t`, do not use that same instance as evidence
        // for its own superclass obligations. This avoids accepting recursive proofs
        // such as constructing `Eq Int` through the `Ord Int` dictionary currently
        // being checked.
        checker.disabled_instance = Some(inst.id);

        let (context_givens, superclass_results) =
            checker.solve_predicates_with_givens(&superclass_obligations, &inst.context);
        checker.disabled_instance = None;
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
        let (next_delta, holes, warnings) = checker.finish();
        delta = next_delta;
        all_holes.extend(holes);
        all_warnings.extend(warnings);

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

            let mut checker = TypeChecker::with_delta(tables, names, file, delta);

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
            let (next_delta, holes, warnings) = checker.finish();
            delta = next_delta;
            all_holes.extend(holes);
            all_warnings.extend(warnings);
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

    let function_dependencies = function_dependency_groups(&module.func_defs);
    let mut checked_defs_by_index: Vec<Option<TermDefinition<TcMetadata, Span>>> =
        vec![None; module.func_defs.len()];

    for group_indices in function_dependencies.groups {
        let mut checker = TypeChecker::with_delta(tables, names, file, delta);

        let needs_placeholders = function_group_needs_placeholders(
            &group_indices,
            &function_dependencies.adjacency,
        );
        let mut group_ctx = Context::new();
        let mut placeholders = FxHashMap::<DefId, Ty>::default();
        let mut placeholder_schemes = Vec::<(DefId, Option<Scheme>)>::new();

        if needs_placeholders {
            for &index in &group_indices {
                let func_def = &module.func_defs[index];
                if func_def.signature.is_some() {
                    continue;
                }

                let evar = checker.fresh_evar_id();
                let placeholder = TypeChecker::evar_type(evar);
                group_ctx = group_ctx.add_entry(ContextEntry::ExVar(evar, Kind::Star));
                placeholders.insert(func_def.id, placeholder.clone());
                let previous = checker
                    .delta
                    .func_schemes
                    .insert(func_def.id, Scheme::mono(placeholder));
                placeholder_schemes.push((func_def.id, previous));
            }
        }

        let mut group_results = Vec::<(
            usize,
            TermExprNode<TcMetadata, Span>,
            FunctionSignatureSource,
            Range<usize>,
        )>::new();
        let mut group_errors = Vec::new();

        for &index in &group_indices {
            let func_def = &module.func_defs[index];
            let wanted_start = checker.wanted_count();
            let checkpoint = checker.checkpoint();

            match &func_def.signature {
                Some(sig) => {
                    let expected_type = TypeChecker::scheme_to_type(sig);

                    match checker.solve_checking(group_ctx.clone(), &func_def.body, &expected_type)
                    {
                        Ok((final_ctx, elaborated)) => {
                            let unresolved = checker.solve_wanteds_from(wanted_start);

                            if !unresolved.is_empty() {
                                let err = checker.wanteds_error(unresolved);
                                checker.restore(checkpoint);
                                group_errors.push(err);
                                continue;
                            }

                            group_ctx = final_ctx;
                            group_results.push((
                                index,
                                elaborated,
                                FunctionSignatureSource::Explicit(sig.clone()),
                                checker.current_wanted_range_from(wanted_start),
                            ));
                        }

                        Err(err) => {
                            checker.restore(checkpoint);
                            group_errors.push(err);
                            continue;
                        }
                    }
                }

                None => {
                    let inferred = if let Some(expected_type) = placeholders.get(&func_def.id) {
                        checker
                            .solve_checking(group_ctx.clone(), &func_def.body, expected_type)
                            .map(|(final_ctx, elaborated)| {
                                (final_ctx, elaborated, expected_type.clone())
                            })
                    } else {
                        checker.solve_inference(group_ctx.clone(), &func_def.body)
                    };

                    match inferred {
                        Ok((final_ctx, elaborated, inferred_type)) => {
                            group_ctx = final_ctx;
                            group_results.push((
                                index,
                                elaborated,
                                FunctionSignatureSource::Inferred(inferred_type),
                                checker.current_wanted_range_from(wanted_start),
                            ));
                        }

                        Err(err) => {
                            checker.restore(checkpoint);
                            group_errors.push(err);
                            continue;
                        }
                    }
                }
            }
        }

        if !group_errors.is_empty() {
            errors.extend(group_errors);
            restore_func_scheme_placeholders(&mut checker.delta.func_schemes, placeholder_schemes);
            checker.finalize_holes();
            let (next_delta, holes, warnings) = checker.finish();
            delta = next_delta;
            all_holes.extend(holes);
            all_warnings.extend(warnings);
            continue;
        }

        let mut finalized_defs =
            Vec::<(usize, TermDefinition<TcMetadata, Span>, Scheme)>::new();

        for (index, elaborated, signature_source, wanted_range) in group_results {
            let func_def = &module.func_defs[index];
            let checkpoint = checker.checkpoint();
            let (final_scheme, final_body) = match signature_source {
                FunctionSignatureSource::Explicit(sig) => {
                    let final_scheme = checker.zonk_scheme(&sig);
                    (final_scheme, checker.zonk_expr(&elaborated))
                }
                FunctionSignatureSource::Inferred(inferred_type) => {
                    let generalized = match checker
                        .generalize_top_level_type_with_subst(
                            &group_ctx,
                            &inferred_type,
                            wanted_range.clone(),
                        ) {
                        Ok(generalized) => generalized,
                        Err(err) => {
                            checker.restore(checkpoint);
                            group_errors.push(err);
                            continue;
                        }
                    };
                    let final_scheme = TypeChecker::type_to_scheme(&generalized.ty);
                    let zonked = checker.zonk_expr(&elaborated);
                    let substituted = substitute_generalized_expr(zonked, &generalized.subst);
                    let wrapped = wrap_inferred_quantifiers(
                        checker.names,
                        &checker.delta.names,
                        substituted,
                        &final_scheme,
                        generalized.retained,
                        func_def.body.anno.clone(),
                    );

                    (final_scheme, wrapped)
                }
            };

            let checked_def = TermDefinition {
                id: func_def.id,
                name: func_def.name.clone(),
                short_name: func_def.short_name.clone(),
                signature: Some(final_scheme.clone()),
                body: final_body,
            };

            finalized_defs.push((index, checked_def, final_scheme));
        }

        if !group_errors.is_empty() {
            errors.extend(group_errors);
            restore_func_scheme_placeholders(&mut checker.delta.func_schemes, placeholder_schemes);
            checker.finalize_holes();
            let (next_delta, holes, warnings) = checker.finish();
            delta = next_delta;
            all_holes.extend(holes);
            all_warnings.extend(warnings);
            continue;
        }

        for (index, checked_def, final_scheme) in finalized_defs {
            checker
                .delta
                .func_schemes
                .insert(checked_def.id, final_scheme.clone());
            checked_func_schemes.insert(checked_def.id, final_scheme);
            checked_defs_by_index[index] = Some(checked_def);
        }

        checker.finalize_holes();
        let (next_delta, holes, warnings) = checker.finish();
        delta = next_delta;
        all_holes.extend(holes);
        all_warnings.extend(warnings);
    }

    let checked_defs: Vec<TermDefinition<TcMetadata, Span>> =
        checked_defs_by_index.into_iter().flatten().collect();

    if errors.is_empty() {
        Ok(TCOutput {
            name_scratch: delta.names,
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
    expected_type: &Ty,
    names: &Names,
) -> Result<Option<TermExprNode<TcMetadata, Span>>, TypeError> {
    let env = TypeCheckEnv::from_checked_module(module);
    let mut checker = TypeChecker::with_env(&env, names, file);
    let (_ctx, elaborated) = checker.solve_checking(Context::new(), expr, expected_type)?;
    let unresolved = checker.unresolved_wanteds();

    if unresolved.is_empty() {
        Ok(Some(checker.zonk_expr(&elaborated)))
    } else {
        Ok(None)
    }
}
