fn sanitize_name(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}
fn mangle_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::TyUnit => "Unit".to_string(),
        TypeExpr::TyVar(id) => format!("v_{}", sanitize_name(&tyvar_display_name(*id))),
        TypeExpr::TyExVar(id) => format!("ev_{}", id.0),
        TypeExpr::TyCon(name) => sanitize_name(name),
        TypeExpr::TyApp(f, x) => format!("App_{}_{}", mangle_type(f), mangle_type(x)),
        TypeExpr::TyArr(a, b) => format!("Arr_{}_{}", mangle_type(a), mangle_type(b)),
        TypeExpr::TyForAll(name, _, body) => {
            format!(
                "Forall_{}_{}",
                sanitize_name(&tyvar_display_name(*name)),
                mangle_type(body)
            )
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
