use std::collections::HashMap;

use solveig_ast::ast::{CtorId, CtorTable, EvarId, PrettyTables, TypeExpr, symbol_display_name};
use solveig_base::FileSpan;
use solveig_base::utils::diagnostics::{Diagnostic, Label};

#[derive(Clone, Debug)]
pub struct HoleInfo {
    pub span: FileSpan,
    pub expected_type: TypeExpr,
    pub context: Vec<(String, TypeExpr)>,
}

impl HoleInfo {
    pub fn suggestions(
        &self,
        constructors: &HashMap<CtorId, TypeExpr>,
        ctor_table: &CtorTable,
    ) -> Vec<String> {
        let mut results = Vec::new();
        results.extend(direct_matches(&self.expected_type, &self.context));
        results.extend(applications(&self.expected_type, &self.context));
        results.extend(constructor_fills(
            &self.expected_type,
            &self.context,
            constructors,
            ctor_table,
        ));
        results.dedup();
        results
    }

    pub fn pretty_type(&self, tables: PrettyTables<'_>) -> String {
        prettify_evars(&self.expected_type).pretty_with_tables(tables)
    }

    pub fn pretty_context(&self, tables: PrettyTables<'_>) -> Vec<(String, String)> {
        self.context
            .iter()
            .map(|(name, ty)| (name.clone(), prettify_evars(ty).pretty_with_tables(tables)))
            .collect()
    }
}

pub fn prettify_evars(ty: &TypeExpr) -> TypeExpr {
    let evars = collect_evars(ty);
    let mut result = ty.clone();
    for (i, evar) in evars.iter().enumerate() {
        let letter = (b'a' + (i as u8 % 26)) as char;
        result = result.substitute_evar(*evar, &TypeExpr::make_var(format!("?{}", letter)));
    }
    result
}

fn collect_evars(ty: &TypeExpr) -> Vec<EvarId> {
    let mut acc = Vec::new();
    collect_evars_inner(ty, &mut acc);
    acc
}

fn collect_evars_inner(ty: &TypeExpr, acc: &mut Vec<EvarId>) {
    match ty {
        TypeExpr::TyExVar(id) => {
            if !acc.contains(id) {
                acc.push(*id);
            }
        }
        TypeExpr::TyArr(a, b) | TypeExpr::TyApp(a, b) => {
            collect_evars_inner(a, acc);
            collect_evars_inner(b, acc);
        }
        TypeExpr::TyForAll(_, preds, body) => {
            for pred in preds {
                for arg in &pred.args {
                    collect_evars_inner(arg, acc);
                }
            }
            collect_evars_inner(body, acc);
        }
        TypeExpr::TyUnit | TypeExpr::TyVar(_) | TypeExpr::TyCon(_) => {}
    }
}

fn types_match(candidate: &TypeExpr, goal: &TypeExpr) -> bool {
    match (candidate, goal) {
        (TypeExpr::TyExVar(_), _) | (_, TypeExpr::TyExVar(_)) => true,

        (TypeExpr::TyForAll(_, _, body), _) => types_match(body, goal),
        (_, TypeExpr::TyForAll(_, _, body)) => types_match(candidate, body),

        (TypeExpr::TyUnit, TypeExpr::TyUnit) => true,
        (TypeExpr::TyVar(a), TypeExpr::TyVar(b)) => a == b,
        (TypeExpr::TyCon(a), TypeExpr::TyCon(b)) => a == b,

        (TypeExpr::TyArr(a1, b1), TypeExpr::TyArr(a2, b2)) => {
            types_match(a1, a2) && types_match(b1, b2)
        }

        (TypeExpr::TyApp(f1, a1), TypeExpr::TyApp(f2, a2)) => {
            types_match(f1, f2) && types_match(a1, a2)
        }

        _ => false,
    }
}

fn heads_match(a: &TypeExpr, b: &TypeExpr) -> bool {
    match (a, b) {
        (TypeExpr::TyForAll(_, _, body), rhs) => heads_match(body, rhs),
        (lhs, TypeExpr::TyForAll(_, _, body)) => heads_match(lhs, body),

        (TypeExpr::TyCon(x), TypeExpr::TyCon(y)) => x == y,
        (TypeExpr::TyApp(f, _), TypeExpr::TyApp(g, _)) => heads_match(f, g),
        (TypeExpr::TyApp(f, _), rhs) | (rhs, TypeExpr::TyApp(f, _)) => heads_match(f, rhs),

        _ => false,
    }
}

fn peel_arrows(ty: &TypeExpr) -> (Vec<TypeExpr>, TypeExpr) {
    let (_, _, body) = ty.peel_scheme_like();
    let mut params = Vec::new();
    let mut current = body;

    while let TypeExpr::TyArr(domain, codomain) = current {
        params.push(*domain);
        current = *codomain;
    }

    (params, current)
}

fn direct_matches(goal: &TypeExpr, ctx: &[(String, TypeExpr)]) -> Vec<String> {
    ctx.iter()
        .filter(|(_, ty)| types_match(ty, goal))
        .map(|(name, _)| name.clone())
        .collect()
}

fn applications(goal: &TypeExpr, ctx: &[(String, TypeExpr)]) -> Vec<String> {
    let mut results = Vec::new();

    for (fname, fty) in ctx {
        let (params, ret) = peel_arrows(fty);
        if params.is_empty() || !types_match(&ret, goal) {
            continue;
        }
        if let Some(args) = fill_params(&params, ctx) {
            results.push(format!("{} {}", fname, args.join(" ")));
        }
    }

    results
}

fn constructor_fills(
    goal: &TypeExpr,
    ctx: &[(String, TypeExpr)],
    constructors: &HashMap<CtorId, TypeExpr>,
    ctor_table: &CtorTable,
) -> Vec<String> {
    let mut results = Vec::new();

    for (id, cty) in constructors {
        let (params, ret) = peel_arrows(cty);
        if !heads_match(&ret, goal) && !types_match(&ret, goal) {
            continue;
        }

        if params.is_empty() {
            results.push(ctor_table_short_name(*id, ctor_table));
        } else if let Some(args) = fill_params(&params, ctx) {
            results.push(format!(
                "{} {}",
                ctor_table_short_name(*id, ctor_table),
                args.join(" ")
            ));
        }
    }

    results
}

fn fill_params(params: &[TypeExpr], ctx: &[(String, TypeExpr)]) -> Option<Vec<String>> {
    params
        .iter()
        .map(|param| {
            ctx.iter()
                .find(|(_, ty)| types_match(ty, param))
                .map(|(name, _)| name.clone())
        })
        .collect()
}

fn ctor_table_short_name(id: CtorId, ctor_table: &CtorTable) -> String {
    ctor_table
        .get(id)
        .map(|info| symbol_display_name(info.short_name))
        .unwrap_or_else(|| panic!("ICE: hole diagnostic constructor table missing CtorId {id:?}"))
}

pub fn hole_diagnostics(
    holes: &[HoleInfo],
    constructor_types: &HashMap<CtorId, TypeExpr>,
    ctor_table: &CtorTable,
    tables: PrettyTables<'_>,
) -> Vec<Diagnostic> {
    holes
        .iter()
        .map(|hole| hole.to_diagnostic(constructor_types, ctor_table, tables))
        .collect()
}

impl HoleInfo {
    pub fn to_diagnostic(
        &self,
        constructor_types: &HashMap<CtorId, TypeExpr>,
        ctor_table: &CtorTable,
        tables: PrettyTables<'_>,
    ) -> Diagnostic {
        let mut diag = Diagnostic::error(
            "HOLE",
            format!("found hole of type `{}`", self.pretty_type(tables),),
        )
        .with_label(Label::primary(
            self.span,
            format!("expects `{}`", self.pretty_type(tables)),
        ));

        let ctx = self.pretty_context(tables);
        if !ctx.is_empty() {
            let bindings = ctx
                .iter()
                .map(|(name, ty)| format!("  {} : {}", name, ty))
                .collect::<Vec<_>>()
                .join("\n");
            diag = diag.with_note(format!("bindings in scope:\n{}", bindings));
        }

        let suggestions = self.suggestions(constructor_types, ctor_table);
        if !suggestions.is_empty() {
            diag = diag.with_note(format!("valid fills include: {}", suggestions.join(", ")));
        }

        diag
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solveig_ast::ast::{
        ClassTable, CtorInfo, CtorTable, Kind, TyConDefinition, TyConId, TyConInfo, TyConTable,
        legacy_symbol_id,
    };
    use solveig_base::{FileId, Span};

    #[test]
    fn hole_diagnostics_use_tables_for_type_and_constructor_names() {
        let tycon_id = TyConId(42);
        let ctor_id = CtorId(7);
        let widget = TypeExpr::TyCon(tycon_id);

        let mut tycon_table = TyConTable::new();
        tycon_table.insert_with_id(
            tycon_id,
            TyConInfo {
                name: legacy_symbol_id("test.pretty.Widget"),
                short_name: legacy_symbol_id("Widget"),
                kind: Kind::Star,
                definition: TyConDefinition::Primitive,
            },
        );

        let mut ctor_table = CtorTable::new();
        ctor_table.insert_with_id(
            ctor_id,
            CtorInfo {
                name: legacy_symbol_id("test.pretty.MakeWidget"),
                short_name: legacy_symbol_id("MakeWidget"),
                tycon: tycon_id,
                fields: Vec::new(),
                full_type: widget.clone(),
                tag: 0,
            },
        );

        let class_table = ClassTable::new();
        let constructors = HashMap::from([(ctor_id, widget.clone())]);
        let hole = HoleInfo {
            span: FileSpan::new(FileId(0), Span::new(0, 0)),
            expected_type: widget,
            context: Vec::new(),
        };
        let diagnostics = hole_diagnostics(
            &[hole],
            &constructors,
            &ctor_table,
            PrettyTables {
                tycons: &tycon_table,
                classes: &class_table,
            },
        );

        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.message, "found hole of type `Widget`");
        assert_eq!(diagnostic.labels[0].message, "expects `Widget`");
        assert!(
            diagnostic
                .notes
                .iter()
                .any(|note| note == "valid fills include: MakeWidget")
        );
    }
}
