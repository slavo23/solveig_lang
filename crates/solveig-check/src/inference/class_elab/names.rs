fn sanitize_name(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}
fn display_tycon_name(id: solveig_ast::ast::TyConId, tables: PrettyTables<'_>) -> String {
    tables
        .tycons
        .get(id)
        .map(|info| symbol_display_name(info.name))
        .unwrap_or_else(|| panic!("ICE: class elaboration type table missing TyConId {id:?}"))
}
fn display_class_name(id: solveig_ast::ast::ClassId, tables: PrettyTables<'_>) -> String {
    tables
        .classes
        .get(id)
        .map(|info| symbol_display_name(info.name))
        .unwrap_or_else(|| panic!("ICE: class elaboration class table missing ClassId {id:?}"))
}
fn mangle_type(ty: &TypeExpr, tables: PrettyTables<'_>) -> String {
    match ty {
        TypeExpr::TyUnit => "Unit".to_string(),
        TypeExpr::TyVar(id) => format!("v_{}", sanitize_name(&tyvar_display_name(*id))),
        TypeExpr::TyExVar(id) => format!("ev_{}", id.0),
        TypeExpr::TyCon(id) => sanitize_name(&display_tycon_name(*id, tables)),
        TypeExpr::TyApp(f, x) => {
            format!("App_{}_{}", mangle_type(f, tables), mangle_type(x, tables))
        }
        TypeExpr::TyArr(a, b) => {
            format!("Arr_{}_{}", mangle_type(a, tables), mangle_type(b, tables))
        }
        TypeExpr::TyForAll(name, _, body) => {
            format!(
                "Forall_{}_{}",
                sanitize_name(&tyvar_display_name(*name)),
                mangle_type(body, tables)
            )
        }
        TypeExpr::Qualified(preds, body) => format!(
            "Qual_{}_{}",
            preds
                .iter()
                .map(|pred| mangle_predicate(pred, tables))
                .collect::<Vec<_>>()
                .join("_"),
            mangle_type(body, tables)
        ),
    }
}
fn mangle_predicate(pred: &Predicate, tables: PrettyTables<'_>) -> String {
    let class = sanitize_name(&display_class_name(pred.class_id, tables));
    let args = pred
        .args
        .iter()
        .map(|arg| mangle_type(arg, tables))
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
fn instance_dict_name(pred: &Predicate, tables: PrettyTables<'_>) -> String {
    format!("dict${}", mangle_predicate(pred, tables))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solveig_ast::ast::{
        ClassDefinition, ClassId, ClassInfo, ClassTable, Kind, TyConDefinition, TyConId, TyConInfo,
        TyConTable, legacy_symbol_id,
    };

    #[test]
    fn mangle_names_use_tables_for_non_legacy_ids() {
        let tycon_id = TyConId(42);
        let class_id = ClassId(77);

        let mut tycon_table = TyConTable::new();
        tycon_table.insert_with_id(
            tycon_id,
            TyConInfo {
                name: legacy_symbol_id("test.pretty.Widget"),
                kind: Kind::Star,
                definition: TyConDefinition::Primitive,
            },
        );

        let mut class_table = ClassTable::new();
        class_table.insert_with_id(
            class_id,
            ClassInfo {
                name: legacy_symbol_id("test.pretty.Showy"),
                definition: ClassDefinition {
                    id: class_id,
                    name: "test.pretty.Showy".to_string(),
                    params: Vec::new(),
                    superclasses: Vec::new(),
                    methods: Vec::new(),
                },
            },
        );

        let tables = PrettyTables {
            tycons: &tycon_table,
            classes: &class_table,
        };
        let pred = Predicate {
            class_id,
            args: vec![TypeExpr::TyCon(tycon_id)],
        };

        assert_eq!(
            instance_dict_name(&pred, tables),
            "dict$test_pretty_Showy_test_pretty_Widget"
        );
    }
}
