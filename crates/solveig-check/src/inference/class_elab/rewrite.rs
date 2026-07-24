fn peel_ev_apps(
    expr: &TermExprNode<TcMetadata, Span>,
) -> &TermExprNode<TcMetadata, Span> {
    let mut head = expr;

    while let TermExpr::EvApp(inner, _) = &head.node {
        head = inner.as_ref();
    }

    head
}

fn rewrite_expr(
    expr: &TermExprNode<TcMetadata, Span>,
    env: &EvidenceEnv,
    class_info: &HashMap<String, ClassRuntimeInfo>,
    method_to_class: &HashMap<String, String>,
    constrained_funcs: &HashMap<String, ConstrainedFuncInfo>,
    instances: &[InstanceDefinition<TcMetadata, Span>],
    tables: PrettyTables<'_>,
) -> TermExprNode<TcMetadata, Span> {
    let anno = expr.anno.clone();

    let node = match &expr.node {
        TermExpr::Var(var_ref) => {
            let name = var_display_name(var_ref);
            let actual_ty = anno.inferred_type.clone();

            if let Some(class_name) = method_to_class.get(&name) {
                let class_rt = class_info
                    .get(class_name)
                    .unwrap_or_else(|| panic!("ICE: missing class runtime info for {}", class_name));

                let method_rt = class_rt
                    .methods
                    .get(&name)
                    .unwrap_or_else(|| panic!("ICE: missing method runtime info for {}", name));

                let mut subst = HashMap::new();

                if match_type_pattern(&method_rt.method_body_ty, &actual_ty, &mut subst) {
                    let needed_pred =
                        instantiate_predicate_with_subst(&method_rt.class_head_pred, &subst);

                    if let Some(dict_expr) = resolve_evidence(
                        &needed_pred,
                        env,
                        class_info,
                        instances,
                        tables,
                        &mut Vec::new(),
                    )
                    {
                        TermExpr::App(
                            Box::new(mk_global_var(&method_rt.selector_name)),
                            Box::new(dict_expr),
                        )
                    } else {
                        panic!("ICE: missing method evidence: {} needs {}",
                            name,
                            needed_pred.pretty_with_tables(tables)
                        );
                    }
                } else {
                    TermExpr::Var(var_ref.clone())
                }
            } else {
                TermExpr::Var(var_ref.clone())
            }
        }

        // C2/C3 bridge: checked core now carries explicit evidence. Until C3
        // replaces this pass with dictionary lowering from Ev, the old
        // class_elab rewrite remains the runtime path and ignores these nodes.
        TermExpr::EvAbs(_, _, body) => {
            return rewrite_expr(
                body,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            );
        }

        TermExpr::EvApp(inner, _) => {
            return rewrite_expr(
                inner,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            );
        }

        TermExpr::Abs(param, ann, body) => TermExpr::Abs(
            param.clone(),
            ann.clone(),
            Box::new(rewrite_expr(
                body,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
        ),

        TermExpr::App(_, _) => {
            let (head, args) = collect_app_spine(expr);

            let rewritten_head = rewrite_expr(
                head,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            );

            let rewritten_args = args
                .iter()
                .map(|arg| {
                    rewrite_expr(
                        arg,
                        env,
                        class_info,
                        method_to_class,
                        constrained_funcs,
                        instances,
                        tables,
                    )
                })
                .collect::<Vec<_>>();

            let lookup_head = peel_ev_apps(head);
            let rebuilt = if let TermExpr::Var(var_ref) = &lookup_head.node {
                let name = var_display_name(var_ref);
                if let Some(info) = constrained_funcs.get(&name) {
                    let actual_head_ty = lookup_head.anno.inferred_type.clone();
                    let mut subst = HashMap::<TyVarId, TypeExpr>::new();

                    if match_type_pattern(&info.body_ty, &actual_head_ty, &mut subst) {
                        let mut dict_args = Vec::new();

                        for pred in &info.predicates {
                            let instantiated_pred = instantiate_predicate_with_subst(pred, &subst);

                            if let Some(dict_expr) = resolve_evidence(
                                &instantiated_pred,
                                env,
                                class_info,
                                instances,
                                tables,
                                &mut Vec::new(),
                            ) {
                                dict_args.push(dict_expr);
                            } else {
                                panic!("ICE: missing constrained-function evidence: {} needs {}",
                                    name,
                                    instantiated_pred.pretty_with_tables(tables)
                                );
                            }
                        }

                        let mut all_args = Vec::new();
                        all_args.extend(dict_args);
                        all_args.extend(rewritten_args);

                        rebuild_app_spine(rewritten_head, all_args)
                    } else {
                        rebuild_app_spine(rewritten_head, rewritten_args)
                    }
                } else {
                    rebuild_app_spine(rewritten_head, rewritten_args)
                }
            } else {
                rebuild_app_spine(rewritten_head, rewritten_args)
            };

            return rebuilt;
        }

        TermExpr::TyAbs(a, k, body) => TermExpr::TyAbs(
            a.clone(),
            k.clone(),
            Box::new(rewrite_expr(
                body,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
        ),

        TermExpr::TyApp(inner, ty) => TermExpr::TyApp(
            Box::new(rewrite_expr(
                inner,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
            ty.clone(),
        ),

        TermExpr::Anno(inner, ty) => TermExpr::Anno(
            Box::new(rewrite_expr(
                inner,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
            ty.clone(),
        ),

        TermExpr::Case(scrutinee, arms) => TermExpr::Case(
            Box::new(rewrite_expr(
                scrutinee,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
            arms.iter()
                .map(|arm| {
                    CaseArmNode::new(
                        CaseArm {
                            pattern: arm.node.pattern.clone(),
                            expr: rewrite_expr(
                                &arm.node.expr,
                                env,
                                class_info,
                                method_to_class,
                                constrained_funcs,
                                instances,
                                tables,
                            ),
                        },
                        arm.anno.clone(),
                    )
                })
                .collect(),
        ),

        TermExpr::IfThenElse(c, t, e) => TermExpr::IfThenElse(
            Box::new(rewrite_expr(
                c,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
            Box::new(rewrite_expr(
                t,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
            Box::new(rewrite_expr(
                e,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
        ),

        TermExpr::BinaryOperator(op, l, r) => TermExpr::BinaryOperator(
            *op,
            Box::new(rewrite_expr(
                l,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
            Box::new(rewrite_expr(
                r,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
        ),

        TermExpr::UnaryOperator(op, rhs) => TermExpr::UnaryOperator(
            *op,
            Box::new(rewrite_expr(
                rhs,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
        ),

        TermExpr::Let(name, binder, in_expr) => TermExpr::Let(
            name.clone(),
            Box::new(rewrite_expr(
                binder,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
            Box::new(rewrite_expr(
                in_expr,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
                tables,
            )),
        ),

        TermExpr::Unit | TermExpr::Lit(_) | TermExpr::Con(_) | TermExpr::Hole => expr.node.clone(),
    };

    TermExprNode::new(node, anno)
}
pub fn elaborate_typeclasses(
    module: &ResolvedModule<TcMetadata, Span>,
) -> ResolvedModule<TcMetadata, Span> {
    let class_info = build_class_runtime_info(module);
    let module_tables = pretty_tables_for_module(module);

    let dict_data_defs = synthesize_dict_data_defs(module, &class_info, module_tables);
    let selector_defs = synthesize_selector_defs(module, &class_info, module_tables);
    let instance_dict_defs = synthesize_instance_dict_defs(module, &class_info, module_tables);

    let mut data_defs = module.data_defs.clone();
    data_defs.extend(dict_data_defs.clone());

    let mut constructor_types = module.constructor_types.clone();
    for dd in &dict_data_defs {
        for ctor in &dd.constructors {
            constructor_types.insert(ctor.id, ctor.full_type.clone());
        }
    }

    let mut tycon_kinds = module.tycon_kinds.clone();
    for dd in &dict_data_defs {
        tycon_kinds.insert(legacy_tycon_id(&dd.name), dd.tycon_kind.clone());
    }

    let mut func_defs = module.func_defs.clone();
    func_defs.extend(selector_defs);
    func_defs.extend(instance_dict_defs);

    let mut rewrite_module = ResolvedModule {
        data_defs: data_defs.clone(),
        class_defs: module.class_defs.clone(),
        instance_defs: module.instance_defs.clone(),
        tycon_table: Default::default(),
        ctor_table: Default::default(),
        class_table: Default::default(),
        tycon_kinds: tycon_kinds.clone(),
        constructor_types: constructor_types.clone(),
        func_schemes: module.func_schemes.clone(),
        func_defs,
    };
    rewrite_module.rebuild_identity_tables();

    let rewritten_func_defs = elaborate_func_defs(&rewrite_module, &class_info);

    let mut output = ResolvedModule {
        data_defs,
        class_defs: Vec::new(),
        instance_defs: Vec::new(),
        tycon_table: Default::default(),
        ctor_table: Default::default(),
        class_table: Default::default(),
        tycon_kinds,
        constructor_types,
        func_schemes: module.func_schemes.clone(),
        func_defs: rewritten_func_defs,
    };
    output.rebuild_identity_tables();
    output
}
