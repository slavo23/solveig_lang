fn dict_type_for_pred(pred: &Predicate, classes: &HashMap<String, ClassRuntimeInfo>) -> TypeExpr {
    let class_name = class_display_name(pred.class_id);
    let info = classes
        .get(&class_name)
        .unwrap_or_else(|| panic!("ICE: missing runtime class info for {}", class_name));

    pred.args
        .iter()
        .cloned()
        .fold(TypeExpr::make_con(info.dict_tycon.clone()), |acc, arg| {
            TypeExpr::make_app(acc, arg)
        })
}
fn mk_anno() -> TcMetadata {
    TcMetadata {
        span: 0..0,
        inferred_type: TypeExpr::TyUnit,
    }
}
fn mk_var(name: &str) -> TermExprNode<TcMetadata, Span> {
    TermExprNode::new(TermExpr::Var(VarRef::legacy_local(name)), mk_anno())
}
fn mk_global_var(name: &str) -> TermExprNode<TcMetadata, Span> {
    TermExprNode::new(TermExpr::Var(VarRef::legacy_global(name)), mk_anno())
}
fn mk_con(name: &str) -> TermExprNode<TcMetadata, Span> {
    TermExprNode::new(TermExpr::Con(legacy_ctor_id(name)), mk_anno())
}
fn mk_app(
    f: TermExprNode<TcMetadata, Span>,
    x: TermExprNode<TcMetadata, Span>,
) -> TermExprNode<TcMetadata, Span> {
    TermExprNode::new(TermExpr::App(Box::new(f), Box::new(x)), mk_anno())
}
fn mk_dict_field_projection(
    dict_expr: TermExprNode<TcMetadata, Span>,
    class_rt: &ClassRuntimeInfo,
    field_ix: usize,
) -> TermExprNode<TcMetadata, Span> {
    let selected_name = "selected".to_string();

    let field_patterns = (0..class_rt.field_count)
        .map(|ix| {
            if ix == field_ix {
                PatternNode::new(Pattern::Var(selected_name.clone()), 0..0)
            } else {
                PatternNode::new(Pattern::Wild, 0..0)
            }
        })
        .collect::<Vec<_>>();

    let dict_pat = PatternNode::new(
        Pattern::Con(legacy_ctor_id(&class_rt.dict_ctor), field_patterns),
        0..0,
    );

    TermExprNode::new(
        TermExpr::Case(
            Box::new(dict_expr),
            vec![CaseArmNode::new(
                CaseArm {
                    pattern: dict_pat,
                    expr: mk_var(&selected_name),
                },
                0..0,
            )],
        ),
        mk_anno(),
    )
}
fn subst_from_class_head(
    params: &[(String, Kind)],
    args: &[TypeExpr],
) -> HashMap<TyVarId, TypeExpr> {
    params
        .iter()
        .zip(args.iter())
        .map(|((param_name, _), arg)| (legacy_tyvar_id(param_name), arg.clone()))
        .collect()
}
fn collect_app_spine(
    expr: &TermExprNode<TcMetadata, Span>,
) -> (
    &TermExprNode<TcMetadata, Span>,
    Vec<&TermExprNode<TcMetadata, Span>>,
) {
    let mut args = Vec::new();
    let mut head = expr;

    while let TermExpr::App(f, x) = &head.node {
        args.push(x.as_ref());
        head = f.as_ref();
    }

    args.reverse();
    (head, args)
}
fn rebuild_app_spine(
    head: TermExprNode<TcMetadata, Span>,
    args: Vec<TermExprNode<TcMetadata, Span>>,
) -> TermExprNode<TcMetadata, Span> {
    args.into_iter().fold(head, |acc, arg| {
        TermExprNode::new(TermExpr::App(Box::new(acc), Box::new(arg)), mk_anno())
    })
}
fn match_type_pattern(
    pattern: &TypeExpr,
    actual: &TypeExpr,
    subst: &mut HashMap<TyVarId, TypeExpr>,
) -> bool {
    match pattern {
        TypeExpr::TyVar(id) => match subst.get(id) {
            Some(existing) => existing == actual,
            None => {
                subst.insert(*id, actual.clone());
                true
            }
        },

        TypeExpr::TyCon(a) => matches!(actual, TypeExpr::TyCon(b) if a == b),
        TypeExpr::TyUnit => matches!(actual, TypeExpr::TyUnit),

        TypeExpr::TyApp(f1, a1) => match actual {
            TypeExpr::TyApp(f2, a2) => {
                match_type_pattern(f1, f2, subst) && match_type_pattern(a1, a2, subst)
            }
            _ => false,
        },

        TypeExpr::TyArr(a1, b1) => match actual {
            TypeExpr::TyArr(a2, b2) => {
                match_type_pattern(a1, a2, subst) && match_type_pattern(b1, b2, subst)
            }
            _ => false,
        },

        TypeExpr::TyForAll(_, _, body) => match_type_pattern(body, actual, subst),
        TypeExpr::Qualified(_, body) => match_type_pattern(body, actual, subst),

        TypeExpr::TyExVar(name) => matches!(actual, TypeExpr::TyExVar(other) if name == other),
    }
}
fn match_predicate_pattern(
    pattern: &Predicate,
    actual: &Predicate,
) -> Option<HashMap<TyVarId, TypeExpr>> {
    if pattern.class_id != actual.class_id || pattern.args.len() != actual.args.len() {
        return None;
    }

    let mut subst = HashMap::<TyVarId, TypeExpr>::new();

    for (pattern_arg, actual_arg) in pattern.args.iter().zip(actual.args.iter()) {
        if !match_type_pattern(pattern_arg, actual_arg, &mut subst) {
            return None;
        }
    }

    Some(subst)
}
fn instantiate_predicate_with_subst(
    pred: &Predicate,
    subst: &HashMap<TyVarId, TypeExpr>,
) -> Predicate {
    Predicate {
        class_id: pred.class_id,
        args: pred
            .args
            .iter()
            .map(|arg| {
                subst
                    .iter()
                    .fold(arg.clone(), |acc, (name, ty)| acc.substitute(*name, ty))
            })
            .collect(),
    }
}
fn resolve_superclass_evidence_from_given(
    wanted: &Predicate,
    given: &Predicate,
    given_expr: TermExprNode<TcMetadata, Span>,
    classes: &HashMap<String, ClassRuntimeInfo>,
    seen: &mut Vec<String>,
) -> Option<TermExprNode<TcMetadata, Span>> {
    if given == wanted {
        return Some(given_expr);
    }

    let key = format!("{}=>{}", mangle_predicate(given), mangle_predicate(wanted));

    if seen.contains(&key) {
        return None;
    }

    seen.push(key);

    let given_class_name = class_display_name(given.class_id);
    let Some(class_rt) = classes.get(&given_class_name) else {
        seen.pop();
        return None;
    };

    if class_rt.params.len() != given.args.len() {
        seen.pop();
        return None;
    }

    let subst = subst_from_class_head(&class_rt.params, &given.args);

    for (super_ix, superclass_pred) in class_rt.superclasses.iter().enumerate() {
        let instantiated_superclass = instantiate_predicate_with_subst(superclass_pred, &subst);

        let superclass_expr = mk_dict_field_projection(given_expr.clone(), class_rt, super_ix);

        if let Some(evidence) = resolve_superclass_evidence_from_given(
            wanted,
            &instantiated_superclass,
            superclass_expr,
            classes,
            seen,
        ) {
            seen.pop();
            return Some(evidence);
        }
    }

    seen.pop();
    None
}
fn resolve_evidence(
    pred: &Predicate,
    env: &EvidenceEnv,
    classes: &HashMap<String, ClassRuntimeInfo>,
    instances: &[InstanceDefinition<TcMetadata, Span>],
    seen: &mut Vec<String>,
) -> Option<TermExprNode<TcMetadata, Span>> {
    if let Some(given) = env.lookup(pred) {
        return Some(given);
    }

    for (given_pred, given_expr) in env.by_pred.iter().rev() {
        if let Some(evidence) = resolve_superclass_evidence_from_given(
            pred,
            given_pred,
            given_expr.clone(),
            classes,
            &mut Vec::new(),
        ) {
            return Some(evidence);
        }
    }

    let key = mangle_predicate(pred);

    if seen.contains(&key) {
        return None;
    }

    seen.push(key);

    for inst in instances {
        let Some(subst) = match_predicate_pattern(&inst.head, pred) else {
            continue;
        };

        let mut dict_expr = mk_global_var(&instance_dict_name(&inst.head));

        for ctx_pred in &inst.context {
            let needed_pred = instantiate_predicate_with_subst(ctx_pred, &subst);

            let ctx_evidence = resolve_evidence(&needed_pred, env, classes, instances, seen)?;

            dict_expr = mk_app(dict_expr, ctx_evidence);
        }

        seen.pop();
        return Some(dict_expr);
    }

    seen.pop();
    None
}
fn build_class_runtime_info(
    module: &ResolvedModule<TcMetadata, Span>,
) -> HashMap<String, ClassRuntimeInfo> {
    let mut out = HashMap::new();

    for class_def in &module.class_defs {
        let class_head_pred = Predicate {
            class_id: class_def.id,
            args: class_def
                .params
                .iter()
                .map(|(name, _)| TypeExpr::make_var(name.clone()))
                .collect(),
        };

        let mut methods = HashMap::new();

        for method in &class_def.methods {
            let short_name = method
                .name
                .rsplit('.')
                .next()
                .unwrap_or(&method.name)
                .to_string();

            let method_info = MethodRuntimeInfo {
                selector_name: selector_name(&method.name),
                class_head_pred: class_head_pred.clone(),
                method_body_ty: method.scheme.ty.clone(),
            };

            methods.insert(short_name.clone(), method_info.clone());
            methods.insert(method.name.clone(), method_info);
        }

        out.insert(
            class_def.name.clone(),
            ClassRuntimeInfo {
                dict_tycon: dict_tycon_name(&class_def.name),
                dict_ctor: dict_ctor_name(&class_def.name),
                params: class_def.params.clone(),
                superclasses: class_def.superclasses.clone(),
                field_count: class_def.superclasses.len() + class_def.methods.len(),
                methods,
            },
        );
    }

    out
}
fn synthesize_dict_data_defs(
    module: &ResolvedModule<TcMetadata, Span>,
    classes: &HashMap<String, ClassRuntimeInfo>,
) -> Vec<DataDefinition> {
    module
        .class_defs
        .iter()
        .map(|class_def| {
            let info = classes
                .get(&class_def.name)
                .expect("ICE: class runtime info must exist for synthesized dictionary data");

            let superclass_field_types = class_def
                .superclasses
                .iter()
                .map(|pred| dict_type_for_pred(pred, classes));

            let method_field_types = class_def.methods.iter().map(|m| m.scheme.ty.clone());

            let field_types = superclass_field_types
                .chain(method_field_types)
                .collect::<Vec<_>>();

            let dict_ty = class_def.params.iter().cloned().fold(
                TypeExpr::make_con(info.dict_tycon.clone()),
                |acc, (name, _)| TypeExpr::make_app(acc, TypeExpr::make_var(name)),
            );

            let ctor_ty = field_types
                .iter()
                .cloned()
                .rfold(dict_ty.clone(), |acc, field_ty| {
                    TypeExpr::make_arr(field_ty, acc)
                });

            DataDefinition {
                name: info.dict_tycon.clone(),
                ty_params: class_def.params.clone(),
                tycon_kind: class_def
                    .params
                    .iter()
                    .fold(Kind::Star, |acc, (_, k)| Kind::make_arrow(k.clone(), acc)),
                constructors: vec![ConstructorDef {
                    id: legacy_ctor_id(&info.dict_ctor),
                    name: info.dict_ctor.clone(),
                    fields: field_types,
                    full_type: ctor_ty,
                }],
            }
        })
        .collect()
}
fn synthesize_selector_defs(
    module: &ResolvedModule<TcMetadata, Span>,
    classes: &HashMap<String, ClassRuntimeInfo>,
) -> Vec<TermDefinition<TcMetadata, Span>> {
    let mut out = Vec::new();

    for class_def in &module.class_defs {
        let info = classes
            .get(&class_def.name)
            .expect("ICE: class runtime info must exist for synthesized selector");

        for (method_ix, method) in class_def.methods.iter().enumerate() {
            let short_name = method.name.rsplit('.').next().unwrap_or(&method.name);
            let selector = info
                .methods
                .get(short_name)
                .expect("ICE: method runtime info must exist for synthesized selector")
                .selector_name
                .clone();
            let dict_arg = "dict".to_string();

            // Superclass dictionaries are stored before method fields.
            let selected_field_ix = class_def.superclasses.len() + method_ix;

            let field_patterns = (0..info.field_count)
                .map(|ix| {
                    let name = if ix == selected_field_ix {
                        "selected".to_string()
                    } else {
                        format!("_field{ix}")
                    };

                    PatternNode::new(Pattern::Var(name), 0..0)
                })
                .collect::<Vec<_>>();

            let dict_pat = PatternNode::new(
                Pattern::Con(legacy_ctor_id(&info.dict_ctor), field_patterns),
                0..0,
            );

            let body = TermExprNode::new(
                TermExpr::Case(
                    Box::new(mk_var(&dict_arg)),
                    vec![CaseArmNode::new(
                        CaseArm {
                            pattern: dict_pat,
                            expr: mk_var("selected"),
                        },
                        0..0,
                    )],
                ),
                mk_anno(),
            );

            let dict_pred = Predicate {
                class_id: class_def.id,
                args: class_def
                    .params
                    .iter()
                    .map(|(name, _)| TypeExpr::make_var(name.clone()))
                    .collect(),
            };

            let selector_ty = TypeExpr::make_arr(
                dict_type_for_pred(&dict_pred, classes),
                method.scheme.ty.clone(),
            );

            let selector_scheme = Scheme {
                quantifiers: class_def
                    .params
                    .iter()
                    .map(|(name, kind)| (legacy_tyvar_id(name), kind.clone()))
                    .collect(),
                predicates: Vec::new(),
                ty: selector_ty,
            };

            out.push(TermDefinition {
                id: legacy_def_id(&selector),
                name: selector,
                signature: Some(selector_scheme),
                body: TermExprNode::new(TermExpr::Abs(dict_arg, None, Box::new(body)), mk_anno()),
            });
        }
    }

    out
}
fn build_method_to_class(module: &ResolvedModule<TcMetadata, Span>) -> HashMap<String, String> {
    let mut method_to_class = HashMap::<String, String>::new();

    for class_def in &module.class_defs {
        for method in &class_def.methods {
            let short_name = method
                .name
                .rsplit('.')
                .next()
                .unwrap_or(&method.name)
                .to_string();

            method_to_class.insert(short_name, class_def.name.clone());
            method_to_class.insert(method.name.clone(), class_def.name.clone());
        }
    }

    method_to_class
}
fn build_constrained_func_info(
    module: &ResolvedModule<TcMetadata, Span>,
) -> HashMap<String, ConstrainedFuncInfo> {
    let mut constrained_funcs = HashMap::<String, ConstrainedFuncInfo>::new();

    for def in &module.func_defs {
        if let Some(sig) = &def.signature
            && !sig.predicates.is_empty()
        {
            let info = ConstrainedFuncInfo {
                predicates: sig.predicates.clone(),
                body_ty: sig.ty.clone(),
            };

            constrained_funcs.insert(def.name.clone(), info.clone());

            let short_name = def.name.rsplit('.').next().unwrap_or(&def.name).to_string();

            constrained_funcs.insert(short_name, info);
        }
    }

    constrained_funcs
}
fn elaborate_func_defs(
    module: &ResolvedModule<TcMetadata, Span>,
    classes: &HashMap<String, ClassRuntimeInfo>,
) -> Vec<TermDefinition<TcMetadata, Span>> {
    let method_to_class = build_method_to_class(module);
    let constrained_funcs = build_constrained_func_info(module);

    module
        .func_defs
        .iter()
        .map(|def| {
            let Some(sig) = &def.signature else {
                return def.clone();
            };

            let mut evidence_env = EvidenceEnv::default();
            let mut new_body = def.body.clone();

            for pred in sig.predicates.iter().rev() {
                let dict_name = instance_dict_name(pred);
                evidence_env.extend(pred.clone(), mk_var(&dict_name));

                let new_body_anno = new_body.anno.clone();
                new_body = TermExprNode::new(
                    TermExpr::Abs(dict_name, None, Box::new(new_body)),
                    new_body_anno,
                );
            }

            new_body = rewrite_expr(
                &new_body,
                &evidence_env,
                classes,
                &method_to_class,
                &constrained_funcs,
                &module.instance_defs,
            );

            let new_ty = sig
                .predicates
                .iter()
                .rev()
                .fold(sig.ty.clone(), |acc, pred| {
                    TypeExpr::make_arr(dict_type_for_pred(pred, classes), acc)
                });

            TermDefinition {
                id: def.id,
                name: def.name.clone(),
                signature: Some(Scheme {
                    quantifiers: sig.quantifiers.clone(),
                    predicates: Vec::new(),
                    ty: new_ty,
                }),
                body: new_body,
            }
        })
        .collect()
}
fn free_type_vars_in_type(ty: &TypeExpr, out: &mut Vec<TyVarId>) {
    match ty {
        TypeExpr::TyVar(id) => {
            if !out.contains(id) {
                out.push(*id);
            }
        }

        TypeExpr::TyArr(lhs, rhs) | TypeExpr::TyApp(lhs, rhs) => {
            free_type_vars_in_type(lhs, out);
            free_type_vars_in_type(rhs, out);
        }

        TypeExpr::TyForAll(name, _, body) => {
            free_type_vars_in_type(body, out);
            out.retain(|v| v != name);
        }

        TypeExpr::Qualified(preds, body) => {
            for pred in preds {
                free_type_vars_in_predicate(pred, out);
            }

            free_type_vars_in_type(body, out);
        }

        TypeExpr::TyUnit | TypeExpr::TyCon(_) | TypeExpr::TyExVar(_) => {}
    }
}
fn free_type_vars_in_predicate(pred: &Predicate, out: &mut Vec<TyVarId>) {
    for arg in &pred.args {
        free_type_vars_in_type(arg, out);
    }
}
fn free_type_vars_in_instance(inst: &InstanceDefinition<TcMetadata, Span>) -> Vec<TyVarId> {
    let mut out = Vec::new();

    for pred in &inst.context {
        free_type_vars_in_predicate(pred, &mut out);
    }

    free_type_vars_in_predicate(&inst.head, &mut out);

    out
}
fn synthesize_instance_dict_defs(
    module: &ResolvedModule<TcMetadata, Span>,
    classes: &HashMap<String, ClassRuntimeInfo>,
) -> Vec<TermDefinition<TcMetadata, Span>> {
    let mut out = Vec::new();

    let method_to_class = build_method_to_class(module);
    let constrained_funcs = build_constrained_func_info(module);

    for inst in &module.instance_defs {
        let dict_name = instance_dict_name(&inst.head);
        let inst_class_name = class_display_name(inst.head.class_id);

        let class_rt = classes
            .get(&inst_class_name)
            .unwrap_or_else(|| panic!("ICE: missing runtime class info for {}", inst_class_name));

        let class_def = module
            .class_defs
            .iter()
            .find(|c| c.id == inst.head.class_id)
            .unwrap_or_else(|| panic!("ICE: missing class definition for {}", inst_class_name));

        let mut evidence_env = EvidenceEnv::default();
        let mut context_dict_names = Vec::new();

        for pred in &inst.context {
            let dict_arg_name = format!("arg${}", instance_dict_name(pred));
            context_dict_names.push(dict_arg_name.clone());
            evidence_env.extend(pred.clone(), mk_var(&dict_arg_name));
        }

        let mut field_exprs = Vec::new();

        let class_subst = subst_from_class_head(&class_def.params, &inst.head.args);

        for superclass_pred in &class_def.superclasses {
            let needed_superclass = instantiate_predicate_with_subst(superclass_pred, &class_subst);

            let superclass_evidence = resolve_evidence(
                &needed_superclass,
                &evidence_env,
                classes,
                &module.instance_defs,
                &mut Vec::new(),
            )
            .unwrap_or_else(|| {
                panic!("ICE: missing superclass evidence: {} requires {}",
                    inst.head.pretty(),
                    needed_superclass.pretty(),
                )
            });

            field_exprs.push(superclass_evidence);
        }

        // Then synthesize ordinary method fields.
        for class_method in &class_def.methods {
            let short_name = class_method
                .name
                .rsplit('.')
                .next()
                .unwrap_or(&class_method.name);

            let impl_method = inst
                .methods
                .iter()
                .find(|m| m.name == short_name)
                .unwrap_or_else(|| {
                    panic!("ICE: missing instance method: {} lacks {}",
                        inst.head.pretty(),
                        class_method.name
                    )
                });

            let rewritten_method = rewrite_expr(
                &impl_method.body,
                &evidence_env,
                classes,
                &method_to_class,
                &constrained_funcs,
                &module.instance_defs,
            );

            field_exprs.push(rewritten_method);
        }

        let mut dict_expr = field_exprs
            .into_iter()
            .fold(mk_con(&class_rt.dict_ctor), mk_app);

        for dict_arg_name in context_dict_names.iter().rev() {
            let anno = dict_expr.anno.clone();
            dict_expr = TermExprNode::new(
                TermExpr::Abs(dict_arg_name.clone(), None, Box::new(dict_expr)),
                anno,
            );
        }

        let dict_ty = dict_type_for_pred(&inst.head, classes);

        let final_ty = inst.context.iter().rev().fold(dict_ty, |acc, pred| {
            TypeExpr::make_arr(dict_type_for_pred(pred, classes), acc)
        });

        let quantifiers = free_type_vars_in_instance(inst)
            .into_iter()
            .map(|name| (name, Kind::Star))
            .collect::<Vec<_>>();

        out.push(TermDefinition {
            id: legacy_def_id(&dict_name),
            name: dict_name,
            signature: Some(Scheme {
                quantifiers,
                predicates: Vec::new(),
                ty: final_ty,
            }),
            body: dict_expr,
        });
    }

    out
}
