//! Hole diagnostics for unresolved source placeholders.
//! The checker records local context and constructor hints for later reporting.

use solveig_base::{FxHashMap, FxHashSet};

use solveig_ast::ast::{CtorId, CtorTable, Names, PrettyTables, Ty, TyVarId};
use solveig_base::FileSpan;
use solveig_base::{Diagnostic, Label, Severity};

#[derive(Clone, Debug)]
pub struct HoleInfo {
    pub span: FileSpan,
    pub expected_type: Ty,
    pub context: Vec<(String, Ty)>,
    pub constructors_in_scope: Vec<CtorId>,
}

impl HoleInfo {
    pub fn suggestions(
        &self,
        constructors: &FxHashMap<CtorId, Ty>,
        ctor_table: &CtorTable,
        names: &Names,
    ) -> Vec<String> {
        if contains_existential(&self.expected_type) {
            return Vec::new();
        }

        let mut results = Vec::new();
        results.extend(direct_matches(&self.expected_type, &self.context));
        results.extend(applications(&self.expected_type, &self.context));
        results.extend(constructor_fills(
            &self.expected_type,
            &self.context,
            &self.constructors_in_scope,
            constructors,
            ctor_table,
            names,
        ));
        let mut seen = FxHashSet::default();
        results.retain(|result| seen.insert(result.clone()));
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

fn contains_existential(ty: &Ty) -> bool {
    match ty {
        Ty::TyExVar(_) => true,
        Ty::TyArr(lhs, rhs) | Ty::TyApp(lhs, rhs) => {
            contains_existential(lhs) || contains_existential(rhs)
        }
        Ty::TyForAll(_, predicates, body) => {
            predicates
                .iter()
                .flat_map(|predicate| &predicate.args)
                .any(contains_existential)
                || contains_existential(body)
        }
        Ty::TyUnit | Ty::TyVar(_) | Ty::TyCon(_) => false,
    }
}

fn peel_arrows(ty: Ty) -> (Vec<Ty>, Ty) {
    let mut params = Vec::new();
    let mut current = ty;

    while let Ty::TyArr(domain, codomain) = current {
        params.push(*domain);
        current = *codomain;
    }

    (params, current)
}

fn direct_matches(goal: &Ty, ctx: &[(String, Ty)]) -> Vec<String> {
    ctx.iter()
        .filter(|(_, ty)| candidate_matches_goal(ty, goal))
        .map(|(name, _)| name.clone())
        .collect()
}

fn applications(goal: &Ty, ctx: &[(String, Ty)]) -> Vec<String> {
    let mut results = Vec::new();

    for (fname, fty) in ctx {
        let Some((quantifiers, body)) = candidate_body(fty) else {
            continue;
        };
        let (params, ret) = peel_arrows(body);
        if params.is_empty() {
            continue;
        }
        let mut substitution = FxHashMap::default();
        if !match_candidate_type(&ret, goal, &quantifiers, &mut substitution) {
            continue;
        }
        if let Some(args) = fill_params(&params, ctx, &quantifiers, &substitution) {
            results.push(format!("{} {}", fname, args.join(" ")));
        }
    }

    results
}

fn constructor_fills(
    goal: &Ty,
    ctx: &[(String, Ty)],
    constructors_in_scope: &[CtorId],
    constructors: &FxHashMap<CtorId, Ty>,
    ctor_table: &CtorTable,
    names: &Names,
) -> Vec<String> {
    let mut results = Vec::new();

    for id in constructors_in_scope {
        let Some(cty) = constructors.get(id) else {
            continue;
        };
        let Some((quantifiers, body)) = candidate_body(cty) else {
            continue;
        };
        let (params, ret) = peel_arrows(body);
        let mut substitution = FxHashMap::default();
        if !match_candidate_type(&ret, goal, &quantifiers, &mut substitution) {
            continue;
        }

        if params.is_empty() {
            results.push(ctor_table_short_name(*id, ctor_table, names));
        } else if let Some(args) = fill_params(&params, ctx, &quantifiers, &substitution) {
            results.push(format!(
                "{} {}",
                ctor_table_short_name(*id, ctor_table, names),
                args.join(" ")
            ));
        }
    }

    results
}

fn fill_params(
    params: &[Ty],
    ctx: &[(String, Ty)],
    quantifiers: &FxHashSet<TyVarId>,
    substitution: &FxHashMap<TyVarId, Ty>,
) -> Option<Vec<String>> {
    params
        .iter()
        .map(|param| {
            let param = substitute_quantified(param, substitution);
            if contains_unbound_quantifier(&param, quantifiers, substitution) {
                return None;
            }
            ctx.iter()
                .find(|(_, ty)| candidate_matches_goal(ty, &param))
                .map(|(name, _)| name.clone())
        })
        .collect()
}

fn candidate_matches_goal(candidate: &Ty, goal: &Ty) -> bool {
    let Some((quantifiers, body)) = candidate_body(candidate) else {
        return false;
    };
    match_candidate_type(&body, goal, &quantifiers, &mut FxHashMap::default())
}

fn candidate_body(candidate: &Ty) -> Option<(FxHashSet<TyVarId>, Ty)> {
    // Following Peyton Jones et al. (JFP 2007), keep the candidate's explicit
    // sigma-type quantifiers together: one instantiation must serve its result
    // and every argument. Predicates are rejected until the hole fact carries
    // enough evidence to prove them rather than guessing an instance.
    let (quantifiers, predicates, body) = candidate.peel_scheme_like();
    predicates
        .is_empty()
        .then(|| (quantifiers.into_iter().map(|(id, _)| id).collect(), body))
}

fn match_candidate_type(
    candidate: &Ty,
    goal: &Ty,
    quantifiers: &FxHashSet<TyVarId>,
    substitution: &mut FxHashMap<TyVarId, Ty>,
) -> bool {
    match (candidate, goal) {
        (Ty::TyVar(id), _) if quantifiers.contains(id) => match substitution.get(id) {
            Some(bound) => bound == goal,
            None => {
                substitution.insert(*id, goal.clone());
                true
            }
        },
        (Ty::TyUnit, Ty::TyUnit) => true,
        (Ty::TyVar(left), Ty::TyVar(right)) => left == right,
        (Ty::TyExVar(left), Ty::TyExVar(right)) => left == right,
        (Ty::TyCon(left), Ty::TyCon(right)) => left == right,
        (Ty::TyArr(left_domain, left_codomain), Ty::TyArr(right_domain, right_codomain))
        | (Ty::TyApp(left_domain, left_codomain), Ty::TyApp(right_domain, right_codomain)) => {
            match_candidate_type(left_domain, right_domain, quantifiers, substitution)
                && match_candidate_type(left_codomain, right_codomain, quantifiers, substitution)
        }
        (Ty::TyForAll(_, _, _), Ty::TyForAll(_, _, _)) => candidate == goal,
        _ => false,
    }
}

fn substitute_quantified(ty: &Ty, substitution: &FxHashMap<TyVarId, Ty>) -> Ty {
    substitution
        .iter()
        .fold(ty.clone(), |ty, (id, replacement)| {
            ty.substitute(*id, replacement)
        })
}

fn contains_unbound_quantifier(
    ty: &Ty,
    quantifiers: &FxHashSet<TyVarId>,
    substitution: &FxHashMap<TyVarId, Ty>,
) -> bool {
    match ty {
        Ty::TyVar(id) => quantifiers.contains(id) && !substitution.contains_key(id),
        Ty::TyArr(lhs, rhs) | Ty::TyApp(lhs, rhs) => {
            contains_unbound_quantifier(lhs, quantifiers, substitution)
                || contains_unbound_quantifier(rhs, quantifiers, substitution)
        }
        Ty::TyForAll(_, predicates, body) => {
            predicates
                .iter()
                .flat_map(|predicate| &predicate.args)
                .any(|arg| contains_unbound_quantifier(arg, quantifiers, substitution))
                || contains_unbound_quantifier(body, quantifiers, substitution)
        }
        Ty::TyUnit | Ty::TyCon(_) | Ty::TyExVar(_) => false,
    }
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
            constructors_in_scope: vec![ctor_id],
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
