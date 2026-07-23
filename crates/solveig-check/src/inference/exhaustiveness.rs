use std::collections::{HashMap, HashSet};
use std::ops::Range;

use solveig_ast::ast::{
    CaseArmNode, CtorId, CtorTable, Literal, Pattern, PatternNode, Predicate, PrettyTables,
    TyConId, TyVarId, TypeExpr, symbol_display_name,
};
use solveig_base::utils::diagnostics::TypeError;
use solveig_base::{FileId, FileSpan, Span as SourceSpan};

const MAX_MISSING_EXAMPLES: usize = 3;
type Span = Range<usize>;

/// Maranget-style exhaustiveness checking.
pub fn check_case(
    file: FileId,
    constructor_types: &HashMap<CtorId, TypeExpr>,
    ctor_table: &CtorTable,
    pretty_tables: PrettyTables<'_>,
    scrutinee_type: &TypeExpr,
    arms: &[CaseArmNode<Span>],
    case_span: Span,
) -> Result<(), TypeError> {
    let constructor_env = ConstructorEnv::from_constructor_types(constructor_types, ctor_table);
    let checker = ExhaustivenessChecker {
        constructor_env,
        ctor_table,
        pretty_tables,
    };

    let scrutinee_type = normalize_type(scrutinee_type);

    let mut matrix = arms
        .iter()
        .map(|arm| vec![lower_pattern(&arm.node.pattern)])
        .collect::<Vec<_>>();

    let vector = vec![Pat::Wild];
    let types = vec![scrutinee_type.clone()];

    let mut missing = Vec::new();

    for _ in 0..MAX_MISSING_EXAMPLES {
        let Some(witness) = checker.useful(&matrix, &vector, &types) else {
            break;
        };

        let rendered = checker.pretty_witness_row(&witness);

        if missing.contains(&rendered) {
            break;
        }

        missing.push(rendered);
        matrix.push(witness_to_pattern_row(&witness));
    }

    if missing.is_empty() {
        Ok(())
    } else {
        Err(TypeError::NonExhaustivePatterns {
            span: FileSpan::new(file, SourceSpan::from_usize_range(case_span)),
            scrutinee_type: scrutinee_type.pretty_with_tables(pretty_tables),
            missing,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum LitPat {
    Int(i64),
}

impl LitPat {
    #[allow(dead_code)]
    fn from_literal(lit: &Literal) -> Self {
        match lit {
            Literal::Int(value) => LitPat::Int(*value),
        }
    }

    fn pretty(&self) -> String {
        match self {
            LitPat::Int(value) => value.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Head {
    Constructor(CtorId),
    Literal(LitPat),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Pat {
    Wild,
    Head(Head, Vec<Pat>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Witness {
    Wild,
    Head(Head, Vec<Witness>),
}

impl Witness {
    fn pretty(&self, ctor_table: &CtorTable) -> String {
        match self {
            Witness::Wild => "_".to_string(),

            Witness::Head(Head::Constructor(id), args) => {
                let display_name = ctor_table_name(*id, ctor_table);
                let name = short_name(&display_name);

                if name == "Nil" && args.is_empty() {
                    return "[]".to_string();
                }

                if name == "Cons" && args.len() == 2 {
                    return format!(
                        "{} :: {}",
                        args[0].pretty_atom(ctor_table),
                        args[1].pretty(ctor_table)
                    );
                }

                if args.is_empty() {
                    name.to_string()
                } else {
                    format!(
                        "{} {}",
                        name,
                        args.iter()
                            .map(|arg| arg.pretty_atom(ctor_table))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                }
            }

            Witness::Head(Head::Literal(lit), args) => {
                debug_assert!(args.is_empty());
                lit.pretty()
            }
        }
    }

    fn pretty_atom(&self, ctor_table: &CtorTable) -> String {
        match self {
            Witness::Wild => self.pretty(ctor_table),
            Witness::Head(_, args) if args.is_empty() => self.pretty(ctor_table),
            Witness::Head(_, _) => format!("({})", self.pretty(ctor_table)),
        }
    }
}

#[derive(Clone, Debug)]
struct ConstructorSig {
    arg_types: Vec<TypeExpr>,
    result_type: TypeExpr,
}

#[derive(Clone, Debug, Default)]
struct ConstructorEnv {
    by_name: HashMap<CtorId, ConstructorSig>,
    by_tycon: HashMap<TyConId, Vec<CtorId>>,
}

fn is_open_primitive_type(ty: &TypeExpr, pretty_tables: PrettyTables<'_>) -> bool {
    match normalize_type(ty) {
        TypeExpr::TyCon(id) => matches!(
            short_name(&tycon_table_name(id, pretty_tables)),
            "Int" | "String"
        ),
        TypeExpr::TyApp(lhs, _) => is_open_primitive_type(&lhs, pretty_tables),
        _ => false,
    }
}

impl ConstructorEnv {
    fn from_constructor_types(
        constructor_types: &HashMap<CtorId, TypeExpr>,
        ctor_table: &CtorTable,
    ) -> Self {
        let mut env = ConstructorEnv::default();

        for (constructor_id, constructor_type) in constructor_types {
            let Some((arg_types, result_type)) = constructor_shape(constructor_type) else {
                continue;
            };

            let Some(result_tycon) = type_head_tycon(&result_type) else {
                continue;
            };

            env.by_name.insert(
                *constructor_id,
                ConstructorSig {
                    arg_types,
                    result_type,
                },
            );

            env.by_tycon
                .entry(result_tycon)
                .or_default()
                .push(*constructor_id);
        }

        for constructors in env.by_tycon.values_mut() {
            constructors.sort_by(|left, right| {
                let left_name = ctor_table_name(*left, ctor_table);
                let right_name = ctor_table_name(*right, ctor_table);
                short_name(&left_name)
                    .cmp(short_name(&right_name))
                    .then_with(|| left_name.cmp(&right_name))
            });
        }

        env
    }

    fn constructors_for_type(
        &self,
        ty: &TypeExpr,
        pretty_tables: PrettyTables<'_>,
    ) -> Option<Vec<CtorId>> {
        if is_open_primitive_type(ty, pretty_tables) {
            return None;
        }

        let tycon = type_head_tycon(ty)?;
        self.by_tycon.get(&tycon).cloned()
    }

    fn field_types_for_constructor(
        &self,
        constructor_id: CtorId,
        scrutinee_type: &TypeExpr,
    ) -> Option<Vec<TypeExpr>> {
        let sig = self.by_name.get(&constructor_id)?;

        let mut subst = HashMap::<TyVarId, TypeExpr>::new();

        if !match_type_template(&sig.result_type, scrutinee_type, &mut subst) {
            return None;
        }

        Some(
            sig.arg_types
                .iter()
                .map(|field_type| subst_type_vars(field_type, &subst))
                .collect(),
        )
    }
}

struct ExhaustivenessChecker<'a> {
    constructor_env: ConstructorEnv,
    ctor_table: &'a CtorTable,
    pretty_tables: PrettyTables<'a>,
}

impl ExhaustivenessChecker<'_> {
    fn pretty_witness_row(&self, witness: &[Witness]) -> String {
        match witness {
            [] => "_".to_string(),

            [one] => one.pretty(self.ctor_table),

            many => many
                .iter()
                .map(|witness| witness.pretty(self.ctor_table))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    fn useful(
        &self,
        matrix: &[Vec<Pat>],
        vector: &[Pat],
        types: &[TypeExpr],
    ) -> Option<Vec<Witness>> {
        if matrix.is_empty() {
            return Some(vec![Witness::Wild; vector.len()]);
        }

        if vector.is_empty() {
            return None;
        }

        let first_type = normalize_type(&types[0]);

        match &vector[0] {
            Pat::Head(head, args) => {
                let field_types = self.field_types_for_head(head, &first_type)?;
                let field_count = field_types.len();

                let specialized_matrix = self.specialize_matrix(matrix, head, field_count);

                let mut specialized_vector = args.clone();
                specialized_vector.extend_from_slice(&vector[1..]);

                let mut specialized_types = field_types;
                specialized_types.extend_from_slice(&types[1..]);

                let witness =
                    self.useful(&specialized_matrix, &specialized_vector, &specialized_types)?;

                if witness.len() < field_count {
                    return None;
                }

                let (fields, rest) = witness.split_at(field_count);

                let mut out = Vec::with_capacity(1 + rest.len());
                out.push(Witness::Head(head.clone(), fields.to_vec()));
                out.extend_from_slice(rest);
                Some(out)
            }

            Pat::Wild => self.useful_wild(matrix, vector, types, &first_type),
        }
    }

    fn useful_wild(
        &self,
        matrix: &[Vec<Pat>],
        vector: &[Pat],
        types: &[TypeExpr],
        first_type: &TypeExpr,
    ) -> Option<Vec<Witness>> {
        let Some(constructors) = self
            .constructor_env
            .constructors_for_type(first_type, self.pretty_tables)
        else {
            let rest = self.useful_default_tail(matrix, vector, types)?;

            let mut out = Vec::with_capacity(1 + rest.len());
            out.push(Witness::Wild);
            out.extend(rest);
            return Some(out);
        };

        let present_constructors = self.present_constructors(matrix);
        let has_complete_signature = constructors
            .iter()
            .all(|constructor| present_constructors.contains(constructor));

        if has_complete_signature {
            self.useful_wild_complete_signature(matrix, vector, types, first_type, constructors)
        } else {
            self.useful_wild_incomplete_signature(
                matrix,
                vector,
                types,
                first_type,
                constructors,
                &present_constructors,
            )
        }
    }

    fn useful_wild_complete_signature(
        &self,
        matrix: &[Vec<Pat>],
        vector: &[Pat],
        types: &[TypeExpr],
        first_type: &TypeExpr,
        constructors: Vec<CtorId>,
    ) -> Option<Vec<Witness>> {
        for constructor_id in constructors {
            let head = Head::Constructor(constructor_id);

            let Some(field_types) = self.field_types_for_head(&head, first_type) else {
                continue;
            };

            let field_count = field_types.len();
            let specialized_matrix = self.specialize_matrix(matrix, &head, field_count);

            let mut specialized_vector = vec![Pat::Wild; field_count];
            specialized_vector.extend_from_slice(&vector[1..]);

            let mut specialized_types = field_types;
            specialized_types.extend_from_slice(&types[1..]);

            if let Some(witness) =
                self.useful(&specialized_matrix, &specialized_vector, &specialized_types)
            {
                if witness.len() < field_count {
                    continue;
                }

                let (fields, rest) = witness.split_at(field_count);

                let mut out = Vec::with_capacity(1 + rest.len());
                out.push(Witness::Head(head, fields.to_vec()));
                out.extend_from_slice(rest);
                return Some(out);
            }
        }

        None
    }

    fn useful_wild_incomplete_signature(
        &self,
        matrix: &[Vec<Pat>],
        vector: &[Pat],
        types: &[TypeExpr],
        first_type: &TypeExpr,
        constructors: Vec<CtorId>,
        present_constructors: &HashSet<CtorId>,
    ) -> Option<Vec<Witness>> {
        // This is the important Maranget default-matrix rule.
        //
        // We must first check the default matrix. Wildcard rows in the matrix
        // cover every constructor in this column, even if those constructors
        // are not explicitly present.
        //
        // This is what makes this exhaustive:
        //
        //   case xs of
        //       [] => ...
        //       | y :: ys => ...
        //
        // After specializing the `Cons` branch, the tail column contains `_`.
        // That wildcard row covers both `[]` and `_ :: _`, so the checker must
        // not invent `_ :: []` or `_ :: _ :: _`.
        let rest = self.useful_default_tail(matrix, vector, types)?;

        let head_witness = self
            .first_missing_constructor(&constructors, present_constructors)
            .and_then(|missing_constructor| {
                let head = Head::Constructor(missing_constructor);
                let field_count = self.field_types_for_head(&head, first_type)?.len();

                Some(Witness::Head(head, vec![Witness::Wild; field_count]))
            })
            .unwrap_or(Witness::Wild);

        let mut out = Vec::with_capacity(1 + rest.len());
        out.push(head_witness);
        out.extend(rest);
        Some(out)
    }

    fn useful_default_tail(
        &self,
        matrix: &[Vec<Pat>],
        vector: &[Pat],
        types: &[TypeExpr],
    ) -> Option<Vec<Witness>> {
        debug_assert!(!vector.is_empty());

        let default_matrix = self.default_matrix(matrix);

        self.useful(&default_matrix, &vector[1..], &types[1..])
    }

    fn present_constructors(&self, matrix: &[Vec<Pat>]) -> HashSet<CtorId> {
        let mut present = HashSet::new();

        for row in matrix {
            let Some((head, _tail)) = row.split_first() else {
                continue;
            };

            if let Pat::Head(Head::Constructor(constructor_id), _) = head {
                present.insert(*constructor_id);
            }
        }

        present
    }

    fn first_missing_constructor(
        &self,
        constructors: &[CtorId],
        present_constructors: &HashSet<CtorId>,
    ) -> Option<CtorId> {
        constructors
            .iter()
            .find(|constructor| !present_constructors.contains(*constructor))
            .copied()
    }

    fn field_types_for_head(
        &self,
        head: &Head,
        scrutinee_type: &TypeExpr,
    ) -> Option<Vec<TypeExpr>> {
        match head {
            Head::Constructor(constructor_id) => self
                .constructor_env
                .field_types_for_constructor(*constructor_id, scrutinee_type),

            Head::Literal(_) => Some(Vec::new()),
        }
    }

    fn specialize_matrix(
        &self,
        matrix: &[Vec<Pat>],
        head: &Head,
        field_count: usize,
    ) -> Vec<Vec<Pat>> {
        let mut out = Vec::new();

        for row in matrix {
            let Some((row_head, row_tail)) = row.split_first() else {
                continue;
            };

            match row_head {
                Pat::Wild => {
                    let mut specialized_row = vec![Pat::Wild; field_count];
                    specialized_row.extend_from_slice(row_tail);
                    out.push(specialized_row);
                }

                Pat::Head(row_head, args) if row_head == head => {
                    let mut specialized_row = args.clone();
                    specialized_row.extend_from_slice(row_tail);
                    out.push(specialized_row);
                }

                Pat::Head(_, _) => {}
            }
        }

        out
    }

    fn default_matrix(&self, matrix: &[Vec<Pat>]) -> Vec<Vec<Pat>> {
        let mut out = Vec::new();

        for row in matrix {
            let Some((head, tail)) = row.split_first() else {
                continue;
            };

            if matches!(head, Pat::Wild) {
                out.push(tail.to_vec());
            }
        }

        out
    }
}

fn lower_pattern(pattern: &PatternNode<Span>) -> Pat {
    match &pattern.node {
        Pattern::Wild | Pattern::Var(_) => Pat::Wild,

        Pattern::Lit(lit) => Pat::Head(Head::Literal(LitPat::from_literal(lit)), Vec::new()),

        Pattern::Con(id, args) => Pat::Head(
            Head::Constructor(*id),
            args.iter().map(lower_pattern).collect(),
        ),
    }
}

fn witness_to_pattern_row(witness: &[Witness]) -> Vec<Pat> {
    witness.iter().map(witness_to_pattern).collect()
}

fn witness_to_pattern(witness: &Witness) -> Pat {
    match witness {
        Witness::Wild => Pat::Wild,

        Witness::Head(head, args) => {
            Pat::Head(head.clone(), args.iter().map(witness_to_pattern).collect())
        }
    }
}

fn constructor_shape(ty: &TypeExpr) -> Option<(Vec<TypeExpr>, TypeExpr)> {
    let mut current = strip_type_prefixes(ty.clone());
    let mut args = Vec::new();

    loop {
        match current {
            TypeExpr::TyArr(arg, result) => {
                args.push(strip_type_prefixes(*arg));
                current = strip_type_prefixes(*result);
            }

            result => return Some((args, result)),
        }
    }
}

fn strip_type_prefixes(mut ty: TypeExpr) -> TypeExpr {
    loop {
        match ty {
            TypeExpr::TyForAll(_, _, body) => {
                ty = *body;
            }

            TypeExpr::Qualified(_, body) => {
                ty = *body;
            }

            other => return other,
        }
    }
}

fn normalize_type(ty: &TypeExpr) -> TypeExpr {
    strip_type_prefixes(ty.clone())
}

fn type_head_tycon(ty: &TypeExpr) -> Option<TyConId> {
    match normalize_type(ty) {
        TypeExpr::TyCon(id) => Some(id),

        TypeExpr::TyApp(lhs, _) => type_head_tycon(&lhs),

        TypeExpr::TyUnit
        | TypeExpr::TyVar(_)
        | TypeExpr::TyExVar(_)
        | TypeExpr::TyArr(_, _)
        | TypeExpr::TyForAll(_, _, _)
        | TypeExpr::Qualified(_, _) => None,
    }
}

fn match_type_template(
    template: &TypeExpr,
    actual: &TypeExpr,
    subst: &mut HashMap<TyVarId, TypeExpr>,
) -> bool {
    match (normalize_type(template), normalize_type(actual)) {
        (TypeExpr::TyVar(id), actual) => {
            if let Some(existing) = subst.get(&id) {
                *existing == actual
            } else {
                subst.insert(id, actual);
                true
            }
        }

        (TypeExpr::TyCon(left), TypeExpr::TyCon(right)) => left == right,

        (TypeExpr::TyUnit, TypeExpr::TyUnit) => true,

        (TypeExpr::TyExVar(left), TypeExpr::TyExVar(right)) => left == right,

        (TypeExpr::TyApp(left_fn, left_arg), TypeExpr::TyApp(right_fn, right_arg)) => {
            match_type_template(&left_fn, &right_fn, subst)
                && match_type_template(&left_arg, &right_arg, subst)
        }

        (TypeExpr::TyArr(left_arg, left_ret), TypeExpr::TyArr(right_arg, right_ret)) => {
            match_type_template(&left_arg, &right_arg, subst)
                && match_type_template(&left_ret, &right_ret, subst)
        }

        _ => false,
    }
}

fn subst_type_vars(ty: &TypeExpr, subst: &HashMap<TyVarId, TypeExpr>) -> TypeExpr {
    match ty {
        TypeExpr::TyVar(id) => subst.get(id).cloned().unwrap_or(TypeExpr::TyVar(*id)),

        TypeExpr::TyUnit => TypeExpr::TyUnit,

        TypeExpr::TyCon(id) => TypeExpr::TyCon(*id),

        TypeExpr::TyExVar(id) => TypeExpr::TyExVar(*id),

        TypeExpr::TyArr(lhs, rhs) => {
            TypeExpr::make_arr(subst_type_vars(lhs, subst), subst_type_vars(rhs, subst))
        }

        TypeExpr::TyApp(lhs, rhs) => {
            TypeExpr::make_app(subst_type_vars(lhs, subst), subst_type_vars(rhs, subst))
        }

        TypeExpr::TyForAll(name, kind, body) => {
            let mut scoped_subst = subst.clone();
            scoped_subst.remove(name);

            TypeExpr::make_forall_id(*name, kind.clone(), subst_type_vars(body, &scoped_subst))
        }

        TypeExpr::Qualified(predicates, body) => TypeExpr::make_qualified(
            predicates
                .iter()
                .map(|pred| Predicate {
                    class_id: pred.class_id,
                    args: pred
                        .args
                        .iter()
                        .map(|arg| subst_type_vars(arg, subst))
                        .collect(),
                })
                .collect(),
            subst_type_vars(body, subst),
        ),
    }
}

fn short_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

fn ctor_table_name(id: CtorId, ctor_table: &CtorTable) -> String {
    ctor_table
        .get(id)
        .map(|info| symbol_display_name(info.name))
        .unwrap_or_else(|| panic!("ICE: exhaustiveness constructor table missing CtorId {id:?}"))
}

fn tycon_table_name(id: TyConId, pretty_tables: PrettyTables<'_>) -> String {
    pretty_tables
        .tycons
        .get(id)
        .map(|info| symbol_display_name(info.name))
        .unwrap_or_else(|| panic!("ICE: exhaustiveness type table missing TyConId {id:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solveig_ast::ast::{
        CaseArm, ClassTable, CtorInfo, ExprNode, Kind, TermExpr, TermExprNode, TyConDefinition,
        TyConInfo, TyConTable, legacy_ctor_id, legacy_symbol_id, legacy_tycon_id,
    };

    struct ExhaustivenessCase {
        name: &'static str,
        scrutinee_type: TypeExpr,
        patterns: Vec<Pattern<Span>>,
        expected_missing: Vec<&'static str>,
    }

    #[test]
    fn check_case_table_driven_witnesses() {
        let fixtures = test_constructor_fixtures();

        let cases = vec![
            ExhaustivenessCase {
                name: "bool_complete",
                scrutinee_type: bool_ty(),
                patterns: vec![con("True", vec![]), con("False", vec![])],
                expected_missing: vec![],
            },
            ExhaustivenessCase {
                name: "bool_missing_false",
                scrutinee_type: bool_ty(),
                patterns: vec![con("True", vec![])],
                expected_missing: vec!["False"],
            },
            ExhaustivenessCase {
                name: "maybe_nested_just_missing_false",
                scrutinee_type: maybe_ty(maybe_ty(bool_ty())),
                patterns: vec![
                    con("Nothing", vec![]),
                    con("Just", vec![con("Nothing", vec![])]),
                    con("Just", vec![con("Just", vec![con("True", vec![])])]),
                ],
                expected_missing: vec!["Just (Just False)"],
            },
            ExhaustivenessCase {
                name: "list_missing_singleton",
                scrutinee_type: list_ty(int_ty()),
                patterns: vec![
                    con("Nil", vec![]),
                    con("Cons", vec![wild(), con("Cons", vec![wild(), wild()])]),
                ],
                expected_missing: vec!["_ :: []"],
            },
            ExhaustivenessCase {
                name: "int_literals_need_wildcard",
                scrutinee_type: int_ty(),
                patterns: vec![lit(1), lit(2)],
                expected_missing: vec!["_"],
            },
            ExhaustivenessCase {
                name: "pair_of_bools_missing_false_false",
                scrutinee_type: pair_ty(bool_ty(), bool_ty()),
                patterns: vec![
                    con("MkPair", vec![con("True", vec![]), con("True", vec![])]),
                    con("MkPair", vec![con("True", vec![]), con("False", vec![])]),
                    con("MkPair", vec![con("False", vec![]), con("True", vec![])]),
                ],
                expected_missing: vec!["MkPair False False"],
            },
            ExhaustivenessCase {
                name: "wildcard_first_covers_all",
                scrutinee_type: maybe_ty(bool_ty()),
                patterns: vec![wild(), con("Nothing", vec![]), con("Just", vec![wild()])],
                expected_missing: vec![],
            },
        ];

        for case in cases {
            let actual = missing_patterns(
                &fixtures.constructor_types,
                &fixtures.ctor_table,
                &fixtures.tycon_table,
                &case.scrutinee_type,
                case.patterns,
            );

            assert_eq!(
                actual, case.expected_missing,
                "unexpected missing patterns for {}",
                case.name
            );
        }
    }

    #[test]
    fn check_case_witnesses_use_constructor_table_names() {
        let tycon_id = TyConId(42);
        let hidden_id = CtorId(7);
        let shown_id = CtorId(9);
        let widget = TypeExpr::TyCon(tycon_id);

        let constructor_types =
            HashMap::from([(hidden_id, widget.clone()), (shown_id, widget.clone())]);

        let mut tycon_table = TyConTable::new();
        tycon_table.insert_with_id(
            tycon_id,
            TyConInfo {
                name: legacy_symbol_id("test.pretty.Widget"),
                kind: Kind::Star,
                definition: TyConDefinition::Primitive,
            },
        );

        let mut ctor_table = CtorTable::new();
        ctor_table.insert_with_id(
            hidden_id,
            CtorInfo {
                name: legacy_symbol_id("test.pretty.Hidden"),
                tycon: tycon_id,
                fields: Vec::new(),
                full_type: widget.clone(),
                tag: 0,
            },
        );
        ctor_table.insert_with_id(
            shown_id,
            CtorInfo {
                name: legacy_symbol_id("test.pretty.Shown"),
                tycon: tycon_id,
                fields: Vec::new(),
                full_type: widget.clone(),
                tag: 1,
            },
        );

        let class_table = ClassTable::new();
        let arms = vec![arm(Pattern::Con(hidden_id, Vec::new()))];

        match check_case(
            FileId(0),
            &constructor_types,
            &ctor_table,
            PrettyTables {
                tycons: &tycon_table,
                classes: &class_table,
            },
            &widget,
            &arms,
            span(),
        ) {
            Ok(()) => panic!("ICE: expected non-exhaustive pattern report"),
            Err(TypeError::NonExhaustivePatterns {
                scrutinee_type,
                missing,
                ..
            }) => {
                assert_eq!(scrutinee_type, "Widget");
                assert_eq!(missing, vec!["Shown"]);
            }
            Err(error) => panic!("ICE: expected only exhaustiveness errors, got {error:?}"),
        }
    }

    struct TestConstructorFixtures {
        constructor_types: HashMap<CtorId, TypeExpr>,
        ctor_table: CtorTable,
        tycon_table: TyConTable,
    }

    fn missing_patterns(
        constructor_types: &HashMap<CtorId, TypeExpr>,
        ctor_table: &CtorTable,
        tycon_table: &TyConTable,
        scrutinee_type: &TypeExpr,
        patterns: Vec<Pattern<Span>>,
    ) -> Vec<String> {
        let arms = patterns.into_iter().map(arm).collect::<Vec<_>>();
        let class_table = ClassTable::new();

        match check_case(
            FileId(0),
            constructor_types,
            ctor_table,
            PrettyTables {
                tycons: tycon_table,
                classes: &class_table,
            },
            scrutinee_type,
            &arms,
            span(),
        ) {
            Ok(()) => Vec::new(),
            Err(TypeError::NonExhaustivePatterns { missing, .. }) => missing,
            Err(error) => panic!("ICE: expected only exhaustiveness errors, got {error:?}"),
        }
    }

    fn test_constructor_fixtures() -> TestConstructorFixtures {
        let mut constructors = HashMap::new();

        constructors.insert(legacy_ctor_id("False"), bool_ty());
        constructors.insert(legacy_ctor_id("True"), bool_ty());

        constructors.insert(legacy_ctor_id("Nothing"), forall("a", maybe_ty(tyvar("a"))));
        constructors.insert(
            legacy_ctor_id("Just"),
            forall("a", arr(tyvar("a"), maybe_ty(tyvar("a")))),
        );

        constructors.insert(legacy_ctor_id("Nil"), forall("a", list_ty(tyvar("a"))));
        constructors.insert(
            legacy_ctor_id("Cons"),
            forall(
                "a",
                arr(tyvar("a"), arr(list_ty(tyvar("a")), list_ty(tyvar("a")))),
            ),
        );

        constructors.insert(
            legacy_ctor_id("MkPair"),
            forall(
                "a",
                forall(
                    "b",
                    arr(tyvar("a"), arr(tyvar("b"), pair_ty(tyvar("a"), tyvar("b")))),
                ),
            ),
        );

        let mut tycon_table = TyConTable::new();
        for name in ["Bool", "Int", "Maybe", "List", "Pair"] {
            tycon_table.insert_with_id(
                legacy_tycon_id(name),
                TyConInfo {
                    name: legacy_symbol_id(name),
                    kind: Kind::Star,
                    definition: TyConDefinition::Primitive,
                },
            );
        }

        let mut ctor_table = CtorTable::new();
        insert_ctor(&mut ctor_table, "False", "Bool", 0, &constructors);
        insert_ctor(&mut ctor_table, "True", "Bool", 1, &constructors);
        insert_ctor(&mut ctor_table, "Nothing", "Maybe", 2, &constructors);
        insert_ctor(&mut ctor_table, "Just", "Maybe", 3, &constructors);
        insert_ctor(&mut ctor_table, "Nil", "List", 4, &constructors);
        insert_ctor(&mut ctor_table, "Cons", "List", 5, &constructors);
        insert_ctor(&mut ctor_table, "MkPair", "Pair", 6, &constructors);

        TestConstructorFixtures {
            constructor_types: constructors,
            ctor_table,
            tycon_table,
        }
    }

    fn insert_ctor(
        ctor_table: &mut CtorTable,
        ctor_name: &str,
        tycon_name: &str,
        tag: u16,
        constructor_types: &HashMap<CtorId, TypeExpr>,
    ) {
        let ctor_id = legacy_ctor_id(ctor_name);
        ctor_table.insert_with_id(
            ctor_id,
            CtorInfo {
                name: legacy_symbol_id(ctor_name),
                tycon: legacy_tycon_id(tycon_name),
                fields: Vec::new(),
                full_type: constructor_types
                    .get(&ctor_id)
                    .cloned()
                    .expect("ICE: test constructor table entry must have a type"),
                tag,
            },
        );
    }

    fn arm(pattern: Pattern<Span>) -> CaseArmNode<Span> {
        ExprNode::new(
            CaseArm {
                pattern: pattern_node(pattern),
                expr: TermExprNode::new(TermExpr::Unit, span()),
            },
            span(),
        )
    }

    fn pattern_node(pattern: Pattern<Span>) -> PatternNode<Span> {
        ExprNode::new(pattern, span())
    }

    fn con(name: &str, args: Vec<Pattern<Span>>) -> Pattern<Span> {
        Pattern::Con(
            legacy_ctor_id(name),
            args.into_iter().map(pattern_node).collect(),
        )
    }

    fn wild() -> Pattern<Span> {
        Pattern::Wild
    }

    fn lit(value: i64) -> Pattern<Span> {
        Pattern::Lit(Literal::Int(value))
    }

    fn span() -> Span {
        0..0
    }

    fn bool_ty() -> TypeExpr {
        tycon("Bool")
    }

    fn int_ty() -> TypeExpr {
        tycon("Int")
    }

    fn maybe_ty(arg: TypeExpr) -> TypeExpr {
        app(tycon("Maybe"), arg)
    }

    fn list_ty(arg: TypeExpr) -> TypeExpr {
        app(tycon("List"), arg)
    }

    fn pair_ty(left: TypeExpr, right: TypeExpr) -> TypeExpr {
        app(app(tycon("Pair"), left), right)
    }

    fn tycon(name: &str) -> TypeExpr {
        TypeExpr::make_con(name.to_string())
    }

    fn tyvar(name: &str) -> TypeExpr {
        TypeExpr::make_var(name.to_string())
    }

    fn app(func: TypeExpr, arg: TypeExpr) -> TypeExpr {
        TypeExpr::make_app(func, arg)
    }

    fn arr(arg: TypeExpr, result: TypeExpr) -> TypeExpr {
        TypeExpr::make_arr(arg, result)
    }

    fn forall(name: &str, body: TypeExpr) -> TypeExpr {
        TypeExpr::make_forall(name.to_string(), Kind::Star, body)
    }
}
