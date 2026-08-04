//! Hole diagnostics for unresolved source placeholders.
//! The checker records local context and constructor hints for later reporting.

use solveig_base::FxHashMap;

use solveig_ast::ast::{CtorId, CtorTable, Names, PrettyTables, Ty};
use solveig_base::FileSpan;
use solveig_base::{Diagnostic, Label, Severity};

#[derive(Clone, Debug)]
pub struct HoleInfo {
    pub span: FileSpan,
    pub expected_type: Ty,
    pub context: Vec<(String, Ty)>,
}

impl HoleInfo {
    pub fn suggestions(
        &self,
        constructors: &FxHashMap<CtorId, Ty>,
        ctor_table: &CtorTable,
        names: &Names,
    ) -> Vec<String> {
        let mut results = Vec::new();
        results.extend(direct_matches(&self.expected_type, &self.context));
        results.extend(applications(&self.expected_type, &self.context));
        results.extend(constructor_fills(
            &self.expected_type,
            &self.context,
            constructors,
            ctor_table,
            names,
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

pub fn prettify_evars(ty: &Ty) -> Ty {
    ty.clone()
}

fn types_match(candidate: &Ty, goal: &Ty) -> bool {
    match (candidate, goal) {
        (Ty::TyExVar(_), _) | (_, Ty::TyExVar(_)) | (Ty::TyUnit, Ty::TyUnit) => true,

        (Ty::TyForAll(_, _, body), _) => types_match(body, goal),
        (_, Ty::TyForAll(_, _, body)) => types_match(candidate, body),

        (Ty::TyVar(a), Ty::TyVar(b)) => a == b,
        (Ty::TyCon(a), Ty::TyCon(b)) => a == b,

        (Ty::TyArr(a1, b1), Ty::TyArr(a2, b2)) => types_match(a1, a2) && types_match(b1, b2),

        (Ty::TyApp(f1, a1), Ty::TyApp(f2, a2)) => types_match(f1, f2) && types_match(a1, a2),

        _ => false,
    }
}

fn heads_match(a: &Ty, b: &Ty) -> bool {
    match (a, b) {
        (Ty::TyForAll(_, _, body), rhs) => heads_match(body, rhs),
        (lhs, Ty::TyForAll(_, _, body)) => heads_match(lhs, body),

        (Ty::TyCon(x), Ty::TyCon(y)) => x == y,
        (Ty::TyApp(f, _), Ty::TyApp(g, _)) => heads_match(f, g),
        (Ty::TyApp(f, _), rhs) | (rhs, Ty::TyApp(f, _)) => heads_match(f, rhs),

        _ => false,
    }
}

fn peel_arrows(ty: &Ty) -> (Vec<Ty>, Ty) {
    let (_, _, body) = ty.peel_scheme_like();
    let mut params = Vec::new();
    let mut current = body;

    while let Ty::TyArr(domain, codomain) = current {
        params.push(*domain);
        current = *codomain;
    }

    (params, current)
}

fn direct_matches(goal: &Ty, ctx: &[(String, Ty)]) -> Vec<String> {
    ctx.iter()
        .filter(|(_, ty)| types_match(ty, goal))
        .map(|(name, _)| name.clone())
        .collect()
}

fn applications(goal: &Ty, ctx: &[(String, Ty)]) -> Vec<String> {
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
    goal: &Ty,
    ctx: &[(String, Ty)],
    constructors: &FxHashMap<CtorId, Ty>,
    ctor_table: &CtorTable,
    names: &Names,
) -> Vec<String> {
    let mut results = Vec::new();

    for (id, cty) in constructors {
        let (params, ret) = peel_arrows(cty);
        if !heads_match(&ret, goal) && !types_match(&ret, goal) {
            continue;
        }

        if params.is_empty() {
            results.push(ctor_table_short_name(*id, ctor_table, names));
        } else if let Some(args) = fill_params(&params, ctx) {
            results.push(format!(
                "{} {}",
                ctor_table_short_name(*id, ctor_table, names),
                args.join(" ")
            ));
        }
    }

    results
}

fn fill_params(params: &[Ty], ctx: &[(String, Ty)]) -> Option<Vec<String>> {
    params
        .iter()
        .map(|param| {
            ctx.iter()
                .find(|(_, ty)| types_match(ty, param))
                .map(|(name, _)| name.clone())
        })
        .collect()
}

fn ctor_table_short_name(id: CtorId, ctor_table: &CtorTable, names: &Names) -> String {
    ctor_table.get(id).map_or_else(
        || solveig_base::ice!("hole diagnostic constructor table missing CtorId {id:?}"),
        |info| names.symbol_display_name(info.short_name),
    )
}

pub fn hole_diagnostics(
    holes: &[HoleInfo],
    constructor_types: &FxHashMap<CtorId, Ty>,
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
        constructor_types: &FxHashMap<CtorId, Ty>,
        ctor_table: &CtorTable,
        tables: PrettyTables<'_>,
    ) -> Diagnostic {
        let mut diag = Diagnostic::new(
            "HOLE",
            Severity::Error,
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
                .map(|(name, ty)| format!("  {name} : {ty}"))
                .collect::<Vec<_>>()
                .join("\n");
            diag = diag.with_note(format!("bindings in scope:\n{bindings}"));
        }

        let suggestions = self.suggestions(constructor_types, ctor_table, tables.names);
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
        ClassTable, CtorInfo, CtorTable, Kind, Names, TyConDefinition, TyConId, TyConInfo,
        TyConTable,
    };
    use solveig_base::{FileId, Span};

    #[test]
    fn hole_diagnostics_use_tables_for_type_and_constructor_names() {
        let tycon_id = TyConId(42);
        let ctor_id = CtorId(7);
        let widget = Ty::TyCon(tycon_id);
        let mut names = Names::new();

        let mut tycon_table = TyConTable::new();
        tycon_table.insert_with_id(
            tycon_id,
            TyConInfo {
                name: names.symbol("test.pretty.Widget"),
                short_name: names.symbol("Widget"),
                kind: Kind::Star,
                definition: TyConDefinition::Primitive,
            },
        );

        let mut ctor_table = CtorTable::new();
        ctor_table.insert_with_id(
            ctor_id,
            CtorInfo {
                name: names.symbol("test.pretty.MakeWidget"),
                short_name: names.symbol("MakeWidget"),
                tycon: tycon_id,
                fields: Vec::new(),
                full_type: widget.clone(),
                tag: 0,
            },
        );

        let class_table = ClassTable::new();
        let constructors = FxHashMap::from_iter([(ctor_id, widget.clone())]);
        let hole = HoleInfo {
            span: FileSpan::new(FileId(0), Span::new(0, 0)),
            expected_type: widget,
            context: Vec::new(),
        };
        let diagnostics = hole_diagnostics(
            &[hole],
            &constructors,
            &ctor_table,
            PrettyTables::new(&names, &tycon_table, &class_table),
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
