use std::collections::HashMap;

use base::utils::span::Span;
use core::ast::{
    CaseArm, CaseArmNode, ConstructorDef, DataDefinition, InstanceDefinition, Kind, Pattern,
    PatternNode, Predicate, ResolvedModule, Scheme, TermDefinition, TermExpr, TermExprNode,
    TypeExpr,
};

use crate::inference::type_infer::TcMetadata;

#[derive(Clone, Debug)]
struct MethodRuntimeInfo {
    selector_name: String,
    class_head_pred: Predicate,
    method_body_ty: TypeExpr,
}

#[derive(Clone, Debug)]
struct ClassRuntimeInfo {
    dict_tycon: String,
    dict_ctor: String,

    params: Vec<(String, Kind)>,
    superclasses: Vec<Predicate>,
    field_count: usize,

    methods: HashMap<String, MethodRuntimeInfo>,
}

#[derive(Clone, Debug)]
struct ConstrainedFuncInfo {
    predicates: Vec<Predicate>,
    body_ty: TypeExpr,
}

#[derive(Clone, Debug, Default)]
struct EvidenceEnv {
    by_pred: Vec<(Predicate, TermExprNode<TcMetadata, Span>)>,
}

impl EvidenceEnv {
    fn lookup(&self, pred: &Predicate) -> Option<TermExprNode<TcMetadata, Span>> {
        // Givens are already in the current type scope, so evidence lookup should be exact.
        // Pattern matching is reserved for instance heads.
        self.by_pred
            .iter()
            .find(|(p, _)| p == pred)
            .map(|(_, expr)| expr.clone())
    }

    fn extend(&mut self, pred: Predicate, expr: TermExprNode<TcMetadata, Span>) {
        self.by_pred.push((pred, expr));
    }
}

fn sanitize_name(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn mangle_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::TyUnit => "Unit".to_string(),
        TypeExpr::TyVar(name) => format!("v_{}", sanitize_name(name)),
        TypeExpr::TyExVar(name) => format!("ev_{}", sanitize_name(name)),
        TypeExpr::TyCon(name) => sanitize_name(name),
        TypeExpr::TyApp(f, x) => format!("App_{}_{}", mangle_type(f), mangle_type(x)),
        TypeExpr::TyArr(a, b) => format!("Arr_{}_{}", mangle_type(a), mangle_type(b)),
        TypeExpr::TyForAll(name, _, body) => {
            format!("Forall_{}_{}", sanitize_name(name), mangle_type(body))
        }
        TypeExpr::Qualified(preds, body) => format!(
            "Qual_{}_{}",
            preds
                .iter()
                .map(mangle_predicate)
                .collect::<Vec<_>>()
                .join("_"),
            mangle_type(body)
        ),
    }
}

fn mangle_predicate(pred: &Predicate) -> String {
    let class = sanitize_name(&pred.class_name);
    let args = pred
        .args
        .iter()
        .map(mangle_type)
        .collect::<Vec<_>>()
        .join("_");
    if args.is_empty() {
        class
    } else {
        format!("{}_{}", class, args)
    }
}

fn dict_tycon_name(class_name: &str) -> String {
    format!("tc.{}Dict", sanitize_name(class_name))
}

fn dict_ctor_name(class_name: &str) -> String {
    format!("tc.Mk{}Dict", sanitize_name(class_name))
}

fn selector_name(method_name: &str) -> String {
    format!("{}$sel", method_name)
}

fn instance_dict_name(pred: &Predicate) -> String {
    format!("dict${}", mangle_predicate(pred))
}

fn dict_type_for_pred(pred: &Predicate, classes: &HashMap<String, ClassRuntimeInfo>) -> TypeExpr {
    let info = classes
        .get(&pred.class_name)
        .unwrap_or_else(|| panic!("missing runtime class info for {}", pred.class_name));

    pred.args
        .iter()
        .cloned()
        .fold(TypeExpr::TyCon(info.dict_tycon.clone()), |acc, arg| {
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
    TermExprNode::new(TermExpr::Var(name.to_string()), mk_anno())
}

fn mk_con(name: &str) -> TermExprNode<TcMetadata, Span> {
    TermExprNode::new(TermExpr::Con(name.to_string()), mk_anno())
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
        Pattern::Con(class_rt.dict_ctor.clone(), field_patterns),
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
) -> HashMap<String, TypeExpr> {
    params
        .iter()
        .zip(args.iter())
        .map(|((param_name, _), arg)| (param_name.clone(), arg.clone()))
        .collect()
}

fn collect_app_spine<'a>(
    expr: &'a TermExprNode<TcMetadata, Span>,
) -> (
    &'a TermExprNode<TcMetadata, Span>,
    Vec<&'a TermExprNode<TcMetadata, Span>>,
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
    subst: &mut HashMap<String, TypeExpr>,
) -> bool {
    match pattern {
        TypeExpr::TyVar(name) => match subst.get(name) {
            Some(existing) => existing == actual,
            None => {
                subst.insert(name.clone(), actual.clone());
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

fn predicate_matches(pattern: &Predicate, actual: &Predicate) -> bool {
    if pattern.class_name != actual.class_name || pattern.args.len() != actual.args.len() {
        return false;
    }

    let mut subst = HashMap::<String, TypeExpr>::new();
    pattern
        .args
        .iter()
        .zip(actual.args.iter())
        .all(|(p, a)| match_type_pattern(p, a, &mut subst))
}

fn match_predicate_pattern(
    pattern: &Predicate,
    actual: &Predicate,
) -> Option<HashMap<String, TypeExpr>> {
    if pattern.class_name != actual.class_name || pattern.args.len() != actual.args.len() {
        return None;
    }

    let mut subst = HashMap::<String, TypeExpr>::new();

    for (pattern_arg, actual_arg) in pattern.args.iter().zip(actual.args.iter()) {
        if !match_type_pattern(pattern_arg, actual_arg, &mut subst) {
            return None;
        }
    }

    Some(subst)
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

    let Some(class_rt) = classes.get(&given.class_name) else {
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

        let mut dict_expr = mk_var(&instance_dict_name(&inst.head));

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
            class_name: class_def.name.clone(),
            args: class_def
                .params
                .iter()
                .map(|(name, _)| TypeExpr::TyVar(name.clone()))
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
            let info = classes.get(&class_def.name).unwrap();

            let superclass_field_types = class_def
                .superclasses
                .iter()
                .map(|pred| dict_type_for_pred(pred, classes));

            let method_field_types = class_def.methods.iter().map(|m| m.scheme.ty.clone());

            // Dictionary layout:
            //
            //   MkOrdDict
            //       superEq
            //       less
            //
            // Superclass dictionaries always come before method fields.
            let field_types = superclass_field_types
                .chain(method_field_types)
                .collect::<Vec<_>>();

            let dict_ty = class_def.params.iter().cloned().fold(
                TypeExpr::TyCon(info.dict_tycon.clone()),
                |acc, (name, _)| TypeExpr::make_app(acc, TypeExpr::TyVar(name)),
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
        let info = classes.get(&class_def.name).unwrap();

        for (method_ix, method) in class_def.methods.iter().enumerate() {
            let short_name = method.name.rsplit('.').next().unwrap_or(&method.name);
            let selector = info.methods.get(short_name).unwrap().selector_name.clone();
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

            let dict_pat =
                PatternNode::new(Pattern::Con(info.dict_ctor.clone(), field_patterns), 0..0);

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
                class_name: class_def.name.clone(),
                args: class_def
                    .params
                    .iter()
                    .map(|(name, _)| TypeExpr::TyVar(name.clone()))
                    .collect(),
            };

            let selector_ty = TypeExpr::make_arr(
                dict_type_for_pred(&dict_pred, classes),
                method.scheme.ty.clone(),
            );

            let selector_scheme = Scheme {
                quantifiers: class_def.params.clone(),
                predicates: Vec::new(),
                ty: selector_ty,
            };

            out.push(TermDefinition {
                name: selector,
                signature: Some(selector_scheme),
                body: TermExprNode::new(TermExpr::Abs(dict_arg, None, Box::new(body)), mk_anno()),
            });
        }
    }

    out
}

fn rewrite_expr(
    expr: &TermExprNode<TcMetadata, Span>,
    env: &EvidenceEnv,
    class_info: &HashMap<String, ClassRuntimeInfo>,
    method_to_class: &HashMap<String, String>,
    constrained_funcs: &HashMap<String, ConstrainedFuncInfo>,
    instances: &[InstanceDefinition<TcMetadata, Span>],
) -> TermExprNode<TcMetadata, Span> {
    let anno = expr.anno.clone();

    let node = match &expr.node {
        TermExpr::Var(name) => {
            let actual_ty = anno.inferred_type.clone();

            if let Some(class_name) = method_to_class.get(name) {
                let class_rt = class_info
                    .get(class_name)
                    .unwrap_or_else(|| panic!("missing class runtime info for {}", class_name));

                let short_name = name.rsplit('.').next().unwrap_or(name);

                let method_rt = class_rt
                    .methods
                    .get(name)
                    .or_else(|| class_rt.methods.get(short_name))
                    .unwrap_or_else(|| panic!("missing method runtime info for {}", name));

                let mut subst = HashMap::new();

                if match_type_pattern(&method_rt.method_body_ty, &actual_ty, &mut subst) {
                    let needed_pred =
                        instantiate_predicate_with_subst(&method_rt.class_head_pred, &subst);

                    if let Some(dict_expr) =
                        resolve_evidence(&needed_pred, env, class_info, instances, &mut Vec::new())
                    {
                        TermExpr::App(
                            Box::new(mk_var(&method_rt.selector_name)),
                            Box::new(dict_expr),
                        )
                    } else {
                        panic!(
                            "missing evidence during class elaboration for method {} and predicate {:?}",
                            name, needed_pred
                        );
                    }
                } else {
                    TermExpr::Var(name.clone())
                }
            } else {
                TermExpr::Var(name.clone())
            }
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
                    )
                })
                .collect::<Vec<_>>();

            let rebuilt = if let TermExpr::Var(name) = &head.node {
                if let Some(info) = constrained_funcs.get(name) {
                    let actual_head_ty = head.anno.inferred_type.clone();
                    let mut subst = HashMap::<String, TypeExpr>::new();

                    if match_type_pattern(&info.body_ty, &actual_head_ty, &mut subst) {
                        let mut dict_args = Vec::new();

                        for pred in &info.predicates {
                            let instantiated_pred = instantiate_predicate_with_subst(pred, &subst);

                            if let Some(dict_expr) = resolve_evidence(
                                &instantiated_pred,
                                env,
                                class_info,
                                instances,
                                &mut Vec::new(),
                            ) {
                                dict_args.push(dict_expr);
                            } else {
                                panic!(
                                    "missing evidence during class elaboration for constrained function {} and predicate {:?}",
                                    name, instantiated_pred
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
            )),
            Box::new(rewrite_expr(
                t,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
            )),
            Box::new(rewrite_expr(
                e,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
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
            )),
            Box::new(rewrite_expr(
                r,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
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
            )),
            Box::new(rewrite_expr(
                in_expr,
                env,
                class_info,
                method_to_class,
                constrained_funcs,
                instances,
            )),
        ),

        TermExpr::Unit | TermExpr::Lit(_) | TermExpr::Con(_) | TermExpr::Hole => expr.node.clone(),
    };

    TermExprNode::new(node, anno)
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
        if let Some(sig) = &def.signature {
            if !sig.predicates.is_empty() {
                let info = ConstrainedFuncInfo {
                    predicates: sig.predicates.clone(),
                    body_ty: sig.ty.clone(),
                };

                constrained_funcs.insert(def.name.clone(), info.clone());

                let short_name = def.name.rsplit('.').next().unwrap_or(&def.name).to_string();

                constrained_funcs.insert(short_name, info);
            }
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

fn free_type_vars_in_type(ty: &TypeExpr, out: &mut Vec<String>) {
    match ty {
        TypeExpr::TyVar(name) => {
            if !out.contains(name) {
                out.push(name.clone());
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

fn free_type_vars_in_predicate(pred: &Predicate, out: &mut Vec<String>) {
    for arg in &pred.args {
        free_type_vars_in_type(arg, out);
    }
}

fn free_type_vars_in_instance(inst: &InstanceDefinition<TcMetadata, Span>) -> Vec<String> {
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

        let class_rt = classes
            .get(&inst.head.class_name)
            .unwrap_or_else(|| panic!("missing runtime class info for {}", inst.head.class_name));

        let class_def = module
            .class_defs
            .iter()
            .find(|c| c.name == inst.head.class_name)
            .unwrap_or_else(|| panic!("missing class definition for {}", inst.head.class_name));

        let mut evidence_env = EvidenceEnv::default();
        let mut context_dict_names = Vec::new();

        for pred in &inst.context {
            let dict_arg_name = format!("arg${}", instance_dict_name(pred));
            context_dict_names.push(dict_arg_name.clone());
            evidence_env.extend(pred.clone(), mk_var(&dict_arg_name));
        }

        let mut field_exprs = Vec::new();

        let class_subst = subst_from_class_head(&class_def.params, &inst.head.args);

        // First synthesize superclass dictionary fields.
        //
        // For:
        //
        //   class Eq a => Ord a where ...
        //   instance Ord Int where ...
        //
        // the Ord Int dictionary begins with evidence for Eq Int.
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
                panic!(
                    "missing superclass evidence during class elaboration: instance {} requires {}",
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
                    panic!(
                        "instance for {} missing method {}",
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
            .fold(mk_con(&class_rt.dict_ctor), |acc, field| mk_app(acc, field));

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

pub fn elaborate_typeclasses(
    module: &ResolvedModule<TcMetadata, Span>,
) -> ResolvedModule<TcMetadata, Span> {
    let class_info = build_class_runtime_info(module);

    let dict_data_defs = synthesize_dict_data_defs(module, &class_info);
    let selector_defs = synthesize_selector_defs(module, &class_info);
    let instance_dict_defs = synthesize_instance_dict_defs(module, &class_info);

    let mut data_defs = module.data_defs.clone();
    data_defs.extend(dict_data_defs.clone());

    let mut constructor_types = module.constructor_types.clone();
    for dd in &dict_data_defs {
        for ctor in &dd.constructors {
            constructor_types.insert(ctor.name.clone(), ctor.full_type.clone());
        }
    }

    let mut tycon_kinds = module.tycon_kinds.clone();
    for dd in &dict_data_defs {
        tycon_kinds.insert(dd.name.clone(), dd.tycon_kind.clone());
    }

    let mut func_defs = module.func_defs.clone();
    func_defs.extend(selector_defs);
    func_defs.extend(instance_dict_defs);

    let rewritten_func_defs = elaborate_func_defs(
        &ResolvedModule {
            data_defs: data_defs.clone(),
            class_defs: module.class_defs.clone(),
            instance_defs: module.instance_defs.clone(),
            tycon_kinds: tycon_kinds.clone(),
            constructor_types: constructor_types.clone(),
            func_schemes: module.func_schemes.clone(),
            func_defs,
        },
        &class_info,
    );

    ResolvedModule {
        data_defs,
        class_defs: Vec::new(),
        instance_defs: Vec::new(),
        tycon_kinds,
        constructor_types,
        func_schemes: module.func_schemes.clone(),
        func_defs: rewritten_func_defs,
    }
}
