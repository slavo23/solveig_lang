use std::collections::{HashMap, HashSet};
use std::ops::Range;

use solveig_ast::ast::{
    CaseArmNode, CtorId, CtorInfo, CtorTable, DataDefinition, Literal, Names, PRIM_TYS, Pattern,
    PatternNode, Predicate, PrettyTables, TyConDefinition, TyConId, TyConTable, TyVarId, TypeExpr,
};
use solveig_base::utils::diagnostics::TypeError;
use solveig_base::{FileId, FileSpan, Span as SourceSpan};

const MAX_MISSING_EXAMPLES: usize = 3;
type Span = Range<usize>;

/// Maranget-style exhaustiveness checking.
pub fn check_case(
    file: FileId,
    tycon_table: &TyConTable,
    ctor_table: &CtorTable,
    pretty_tables: PrettyTables<'_>,
    scrutinee_type: &TypeExpr,
    arms: &[CaseArmNode<Span>],
    case_span: Span,
) -> CaseDiagnostics {
    let data_env = DataEnv {
        names: pretty_tables.names,
        tycon_table,
        ctor_table,
    };
    let checker = ExhaustivenessChecker {
        data_env,
        ctor_table,
    };

    let scrutinee_type = normalize_type(scrutinee_type);

    let matrix = arms
        .iter()
        .map(|arm| vec![lower_pattern(&arm.node.pattern)])
        .collect::<Vec<_>>();

    let vector = vec![Pat::Wild];
    let types = vec![scrutinee_type.clone()];

    let warnings = unreachable_pattern_warnings(file, &checker, &matrix, &types, arms);
    let mut exhaustiveness_matrix = matrix;
    let mut missing = Vec::new();

    for _ in 0..MAX_MISSING_EXAMPLES {
        let Some(witness) = checker.useful(&exhaustiveness_matrix, &vector, &types) else {
            break;
        };

        let rendered = checker.pretty_witness_row(&witness);

        if missing.contains(&rendered) {
            break;
        }

        missing.push(rendered);
        exhaustiveness_matrix.push(witness_to_pattern_row(&witness));
    }

    let error = if missing.is_empty() {
        None
    } else {
        Some(TypeError::NonExhaustivePatterns {
            span: FileSpan::new(file, SourceSpan::from_usize_range(case_span)),
            scrutinee_type: scrutinee_type.pretty_with_tables(pretty_tables),
            missing,
        })
    };

    CaseDiagnostics { warnings, error }
}

#[derive(Clone, Debug, Default)]
pub struct CaseDiagnostics {
    pub warnings: Vec<TypeError>,
    pub error: Option<TypeError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum LitPat {
    Int(i64),
    Float(u64),
    Char(char),
    Str(solveig_ast::ast::StrLitId),
}

impl LitPat {
    #[allow(dead_code)]
    fn from_literal(lit: &Literal) -> Self {
        match lit {
            Literal::Int(value) => LitPat::Int(*value),
            Literal::Float(value) => LitPat::Float(value.to_bits()),
            Literal::Char(value) => LitPat::Char(*value),
            Literal::Str(id) => LitPat::Str(*id),
        }
    }

    fn pretty(&self, names: &Names) -> String {
        match self {
            LitPat::Int(value) => value.to_string(),
            LitPat::Float(bits) => f64::from_bits(*bits).to_string(),
            LitPat::Char(value) => format!("'{}'", escape_char_literal(*value)),
            LitPat::Str(id) => {
                format!(
                    "\"{}\"",
                    escape_string_literal(names.string_literal_value(*id))
                )
            }
        }
    }
}

fn escape_char_literal(ch: char) -> String {
    match ch {
        '\n' => "\\n".to_string(),
        '\t' => "\\t".to_string(),
        '\\' => "\\\\".to_string(),
        '\'' => "\\'".to_string(),
        '"' => "\\\"".to_string(),
        ch if ch.is_control() => ch.escape_unicode().to_string(),
        ch => ch.to_string(),
    }
}

fn escape_string_literal(value: &str) -> String {
    let mut out = String::new();

    for ch in value.chars() {
        out.push_str(&match ch {
            '\n' => "\\n".to_string(),
            '\t' => "\\t".to_string(),
            '\\' => "\\\\".to_string(),
            '"' => "\\\"".to_string(),
            ch if ch.is_control() => ch.escape_unicode().to_string(),
            ch => ch.to_string(),
        });
    }

    out
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
    fn pretty(&self, names: &Names, ctor_table: &CtorTable) -> String {
        match self {
            Witness::Wild => "_".to_string(),

            Witness::Head(Head::Constructor(id), args) => {
                let name = ctor_table_short_name(names, *id, ctor_table);

                if name == "Nil" && args.is_empty() {
                    return "[]".to_string();
                }

                if name == "Cons" && args.len() == 2 {
                    return format!(
                        "{} :: {}",
                        args[0].pretty_atom(names, ctor_table),
                        args[1].pretty(names, ctor_table)
                    );
                }

                if args.is_empty() {
                    name
                } else {
                    format!(
                        "{} {}",
                        name,
                        args.iter()
                            .map(|arg| arg.pretty_atom(names, ctor_table))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                }
            }

            Witness::Head(Head::Literal(lit), args) => {
                debug_assert!(args.is_empty());
                lit.pretty(names)
            }
        }
    }

    fn pretty_atom(&self, names: &Names, ctor_table: &CtorTable) -> String {
        match self {
            Witness::Wild => self.pretty(names, ctor_table),
            Witness::Head(_, args) if args.is_empty() => self.pretty(names, ctor_table),
            Witness::Head(_, _) => format!("({})", self.pretty(names, ctor_table)),
        }
    }
}

fn is_open_primitive_type(ty: &TypeExpr) -> bool {
    type_head_tycon(ty).is_some_and(|id| PRIM_TYS.open_primitive_tycon_ids().contains(&id))
}

#[derive(Clone, Copy)]
struct DataEnv<'a> {
    names: &'a Names,
    tycon_table: &'a TyConTable,
    ctor_table: &'a CtorTable,
}

impl DataEnv<'_> {
    fn constructors_for_type(&self, ty: &TypeExpr) -> Option<Vec<CtorId>> {
        if is_open_primitive_type(ty) {
            return None;
        }

        let (tycon, _) = type_head_and_args(ty)?;
        let data_def = self.data_definition(tycon)?;

        Some(
            data_def
                .constructors
                .iter()
                .map(|constructor| constructor.id)
                .collect(),
        )
    }

    fn field_types_for_constructor(
        &self,
        constructor_id: CtorId,
        scrutinee_type: &TypeExpr,
    ) -> Option<Vec<TypeExpr>> {
        let ctor = self.ctor_table.get(constructor_id)?;
        let (tycon, ty_args) = type_head_and_args(scrutinee_type)?;

        if ctor.tycon != tycon {
            return None;
        }

        let data_def = self.data_definition(ctor.tycon)?;
        Some(instantiate_ctor_fields(data_def, ctor, &ty_args))
    }

    fn data_definition(&self, tycon: TyConId) -> Option<&DataDefinition> {
        let tycon_info = self.tycon_table.get(tycon)?;

        match &tycon_info.definition {
            TyConDefinition::Data(data_def) => Some(data_def),
            TyConDefinition::Primitive => None,
        }
    }
}

struct ExhaustivenessChecker<'a> {
    data_env: DataEnv<'a>,
    ctor_table: &'a CtorTable,
}

impl ExhaustivenessChecker<'_> {
    fn pretty_witness_row(&self, witness: &[Witness]) -> String {
        match witness {
            [] => "_".to_string(),

            [one] => one.pretty(self.data_env.names, self.ctor_table),

            many => many
                .iter()
                .map(|witness| witness.pretty(self.data_env.names, self.ctor_table))
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
        let Some(constructors) = self.data_env.constructors_for_type(first_type) else {
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
                .data_env
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

fn unreachable_pattern_warnings(
    file: FileId,
    checker: &ExhaustivenessChecker<'_>,
    matrix: &[Vec<Pat>],
    types: &[TypeExpr],
    arms: &[CaseArmNode<Span>],
) -> Vec<TypeError> {
    let mut warnings = Vec::new();

    for (index, row) in matrix.iter().enumerate() {
        if checker.useful(&matrix[..index], row, types).is_none() {
            warnings.push(TypeError::UnreachablePattern {
                span: FileSpan::new(
                    file,
                    SourceSpan::from_usize_range(arms[index].node.pattern.anno.clone()),
                ),
            });
        }
    }

    warnings
}

fn normalize_type(ty: &TypeExpr) -> TypeExpr {
    let mut current = ty;

    while let TypeExpr::TyForAll(_, _, body) = current {
        current = body;
    }

    current.clone()
}

fn type_head_tycon(ty: &TypeExpr) -> Option<TyConId> {
    type_head_and_args(ty).map(|(tycon, _)| tycon)
}

fn type_head_and_args(ty: &TypeExpr) -> Option<(TyConId, Vec<TypeExpr>)> {
    let mut current = normalize_type(ty);
    let mut args = Vec::new();

    loop {
        match current {
            TypeExpr::TyCon(id) => {
                args.reverse();
                return Some((id, args));
            }

            TypeExpr::TyApp(lhs, rhs) => {
                args.push(normalize_type(&rhs));
                current = normalize_type(&lhs);
            }

            TypeExpr::TyUnit
            | TypeExpr::TyVar(_)
            | TypeExpr::TyExVar(_)
            | TypeExpr::TyArr(_, _)
            | TypeExpr::TyForAll(_, _, _) => return None,
        }
    }
}

fn instantiate_ctor_fields(
    data_def: &DataDefinition,
    ctor: &CtorInfo,
    ty_args: &[TypeExpr],
) -> Vec<TypeExpr> {
    let subst = data_def
        .ty_params
        .iter()
        .zip(ty_args)
        .map(|((param_id, _), arg)| (*param_id, arg.clone()))
        .collect::<HashMap<_, _>>();

    ctor.fields
        .iter()
        .map(|field_type| subst_type_vars(field_type, &subst))
        .collect()
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

        TypeExpr::TyForAll(binders, predicates, body) => {
            let mut scoped_subst = subst.clone();
            for (name, _) in binders {
                scoped_subst.remove(name);
            }

            TypeExpr::make_forall_group(
                binders.clone(),
                predicates
                    .iter()
                    .map(|pred| Predicate {
                        class_id: pred.class_id,
                        args: pred
                            .args
                            .iter()
                            .map(|arg| subst_type_vars(arg, &scoped_subst))
                            .collect(),
                    })
                    .collect(),
                subst_type_vars(body, &scoped_subst),
            )
        }
    }
}

fn ctor_table_short_name(names: &Names, id: CtorId, ctor_table: &CtorTable) -> String {
    ctor_table
        .get(id)
        .map(|info| names.symbol_display_name(info.short_name))
        .unwrap_or_else(|| panic!("ICE: exhaustiveness constructor table missing CtorId {id:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solveig_ast::ast::{
        CaseArm, ClassTable, ConstructorDef, CtorInfo, ExprNode, Kind, Names, TermExpr,
        TermExprNode, TyConDefinition, TyConInfo, TyConTable,
    };
    use solveig_base::Severity;

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
                &fixtures.names,
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
        let mut names = Names::new();
        let data_def = DataDefinition {
            name: "test.pretty.Widget".to_string(),
            short_name: "Widget".to_string(),
            ty_params: Vec::new(),
            tycon_kind: Kind::Star,
            constructors: vec![
                ConstructorDef {
                    id: hidden_id,
                    name: "test.pretty.Hidden".to_string(),
                    short_name: "Hidden".to_string(),
                    fields: Vec::new(),
                    full_type: widget.clone(),
                },
                ConstructorDef {
                    id: shown_id,
                    name: "test.pretty.Shown".to_string(),
                    short_name: "Shown".to_string(),
                    fields: Vec::new(),
                    full_type: widget.clone(),
                },
            ],
        };

        let mut tycon_table = TyConTable::new();
        tycon_table.insert_with_id(
            tycon_id,
            TyConInfo {
                name: names.symbol("test.pretty.Widget"),
                short_name: names.symbol("Widget"),
                kind: Kind::Star,
                definition: TyConDefinition::Data(data_def),
            },
        );

        let mut ctor_table = CtorTable::new();
        ctor_table.insert_with_id(
            hidden_id,
            CtorInfo {
                name: names.symbol("test.pretty.Hidden"),
                short_name: names.symbol("Hidden"),
                tycon: tycon_id,
                fields: Vec::new(),
                full_type: widget.clone(),
                tag: 0,
            },
        );
        ctor_table.insert_with_id(
            shown_id,
            CtorInfo {
                name: names.symbol("test.pretty.Shown"),
                short_name: names.symbol("Shown"),
                tycon: tycon_id,
                fields: Vec::new(),
                full_type: widget.clone(),
                tag: 1,
            },
        );

        let class_table = ClassTable::new();
        let arms = vec![arm(Pattern::Con(hidden_id, Vec::new()))];

        let diagnostics = check_case(
            FileId(0),
            &tycon_table,
            &ctor_table,
            PrettyTables {
                names: &names,
                tycons: &tycon_table,
                classes: &class_table,
            },
            &widget,
            &arms,
            span(),
        );

        match diagnostics.error {
            Some(TypeError::NonExhaustivePatterns {
                scrutinee_type,
                missing,
                ..
            }) => {
                assert_eq!(scrutinee_type, "Widget");
                assert_eq!(missing, vec!["Shown"]);
            }
            Some(error) => panic!("ICE: expected only exhaustiveness errors, got {error:?}"),
            None => panic!("ICE: expected non-exhaustive pattern report"),
        }
    }

    #[test]
    fn instantiate_ctor_fields_substitutes_scrutinee_type_args() {
        let fixtures = test_constructor_fixtures();
        let data_def = data_def_named(&fixtures.tycon_table, "List");
        let cons = fixtures
            .ctor_table
            .get(ctor_id("Cons"))
            .expect("ICE: test fixture must define Cons");

        let fields = instantiate_ctor_fields(data_def, cons, &[int_ty()]);

        assert_eq!(fields, vec![int_ty(), list_ty(int_ty())]);
    }

    #[test]
    fn check_case_reports_unreachable_pattern_warnings() {
        let duplicate = redundancy_warnings_for_patterns(vec![
            con("True", vec![]),
            con("True", vec![]),
            con("False", vec![]),
        ]);

        assert_eq!(duplicate.len(), 1);
        assert_unreachable_warning(&duplicate[0]);

        let after_wildcard = redundancy_warnings_for_patterns(vec![wild(), con("True", vec![])]);

        assert_eq!(after_wildcard.len(), 1);
        assert_unreachable_warning(&after_wildcard[0]);

        let overlapping_but_useful = redundancy_warnings_for_maybe_bool_patterns(vec![
            con("Just", vec![con("True", vec![])]),
            con("Just", vec![wild()]),
            con("Nothing", vec![]),
        ]);

        assert!(overlapping_but_useful.is_empty());
    }

    struct TestConstructorFixtures {
        names: Names,
        ctor_table: CtorTable,
        tycon_table: TyConTable,
    }

    fn redundancy_warnings_for_patterns(patterns: Vec<Pattern<Span>>) -> Vec<TypeError> {
        check_patterns(bool_ty(), patterns).warnings
    }

    fn redundancy_warnings_for_maybe_bool_patterns(patterns: Vec<Pattern<Span>>) -> Vec<TypeError> {
        check_patterns(maybe_ty(bool_ty()), patterns).warnings
    }

    fn check_patterns(scrutinee_type: TypeExpr, patterns: Vec<Pattern<Span>>) -> CaseDiagnostics {
        let fixtures = test_constructor_fixtures();
        let arms = patterns.into_iter().map(arm).collect::<Vec<_>>();
        let class_table = ClassTable::new();

        check_case(
            FileId(0),
            &fixtures.tycon_table,
            &fixtures.ctor_table,
            PrettyTables {
                names: &fixtures.names,
                tycons: &fixtures.tycon_table,
                classes: &class_table,
            },
            &scrutinee_type,
            &arms,
            span(),
        )
    }

    fn assert_unreachable_warning(error: &TypeError) {
        let TypeError::UnreachablePattern { .. } = error else {
            panic!("ICE: expected unreachable-pattern warning, got {error:?}");
        };

        let diagnostic = error.to_diagnostic();
        assert_eq!(diagnostic.code, "TC0017");
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert_eq!(diagnostic.message, "unreachable pattern");
    }

    fn missing_patterns(
        names: &Names,
        ctor_table: &CtorTable,
        tycon_table: &TyConTable,
        scrutinee_type: &TypeExpr,
        patterns: Vec<Pattern<Span>>,
    ) -> Vec<String> {
        let arms = patterns.into_iter().map(arm).collect::<Vec<_>>();
        let class_table = ClassTable::new();

        match check_case(
            FileId(0),
            tycon_table,
            ctor_table,
            PrettyTables {
                names,
                tycons: tycon_table,
                classes: &class_table,
            },
            scrutinee_type,
            &arms,
            span(),
        )
        .error
        {
            None => Vec::new(),
            Some(TypeError::NonExhaustivePatterns { missing, .. }) => missing,
            Some(error) => panic!("ICE: expected only exhaustiveness errors, got {error:?}"),
        }
    }

    fn test_constructor_fixtures() -> TestConstructorFixtures {
        let mut names = Names::new();
        let mut tycon_table = TyConTable::new();
        let mut ctor_table = CtorTable::new();

        tycon_table.insert_with_id(
            PRIM_TYS.int,
            TyConInfo {
                name: names.symbol("Int"),
                short_name: names.symbol("Int"),
                kind: Kind::Star,
                definition: TyConDefinition::Primitive,
            },
        );

        let data_defs = vec![
            DataDefinition {
                name: "Bool".to_string(),
                short_name: "Bool".to_string(),
                ty_params: Vec::new(),
                tycon_kind: Kind::Star,
                constructors: vec![
                    ConstructorDef {
                        id: ctor_id("True"),
                        name: "True".to_string(),
                        short_name: "True".to_string(),
                        fields: Vec::new(),
                        full_type: bool_ty(),
                    },
                    ConstructorDef {
                        id: ctor_id("False"),
                        name: "False".to_string(),
                        short_name: "False".to_string(),
                        fields: Vec::new(),
                        full_type: bool_ty(),
                    },
                ],
            },
            DataDefinition {
                name: "Maybe".to_string(),
                short_name: "Maybe".to_string(),
                ty_params: vec![(TyVarId(0), Kind::Star)],
                tycon_kind: unary_kind(),
                constructors: vec![
                    ConstructorDef {
                        id: ctor_id("Nothing"),
                        name: "Nothing".to_string(),
                        short_name: "Nothing".to_string(),
                        fields: Vec::new(),
                        full_type: forall("a", maybe_ty(tyvar("a"))),
                    },
                    ConstructorDef {
                        id: ctor_id("Just"),
                        name: "Just".to_string(),
                        short_name: "Just".to_string(),
                        fields: vec![tyvar("a")],
                        full_type: forall("a", arr(tyvar("a"), maybe_ty(tyvar("a")))),
                    },
                ],
            },
            DataDefinition {
                name: "List".to_string(),
                short_name: "List".to_string(),
                ty_params: vec![(TyVarId(0), Kind::Star)],
                tycon_kind: unary_kind(),
                constructors: vec![
                    ConstructorDef {
                        id: ctor_id("Nil"),
                        name: "Nil".to_string(),
                        short_name: "Nil".to_string(),
                        fields: Vec::new(),
                        full_type: forall("a", list_ty(tyvar("a"))),
                    },
                    ConstructorDef {
                        id: ctor_id("Cons"),
                        name: "Cons".to_string(),
                        short_name: "Cons".to_string(),
                        fields: vec![tyvar("a"), list_ty(tyvar("a"))],
                        full_type: forall(
                            "a",
                            arr(tyvar("a"), arr(list_ty(tyvar("a")), list_ty(tyvar("a")))),
                        ),
                    },
                ],
            },
            DataDefinition {
                name: "Pair".to_string(),
                short_name: "Pair".to_string(),
                ty_params: vec![(TyVarId(0), Kind::Star), (TyVarId(1), Kind::Star)],
                tycon_kind: binary_kind(),
                constructors: vec![ConstructorDef {
                    id: ctor_id("MkPair"),
                    name: "MkPair".to_string(),
                    short_name: "MkPair".to_string(),
                    fields: vec![tyvar("a"), tyvar("b")],
                    full_type: forall(
                        "a",
                        forall(
                            "b",
                            arr(tyvar("a"), arr(tyvar("b"), pair_ty(tyvar("a"), tyvar("b")))),
                        ),
                    ),
                }],
            },
        ];

        let mut next_tag = 0;
        for data_def in data_defs {
            insert_data_definition(
                &mut names,
                &mut tycon_table,
                &mut ctor_table,
                data_def,
                &mut next_tag,
            );
        }

        TestConstructorFixtures {
            names,
            ctor_table,
            tycon_table,
        }
    }

    fn insert_data_definition(
        names: &mut Names,
        tycon_table: &mut TyConTable,
        ctor_table: &mut CtorTable,
        data_def: DataDefinition,
        next_tag: &mut u16,
    ) {
        let tycon = tycon_id(&data_def.short_name);

        tycon_table.insert_with_id(
            tycon,
            TyConInfo {
                name: names.symbol(&data_def.name),
                short_name: names.symbol(&data_def.short_name),
                kind: data_def.tycon_kind.clone(),
                definition: TyConDefinition::Data(data_def.clone()),
            },
        );

        for constructor in &data_def.constructors {
            ctor_table.insert_with_id(
                constructor.id,
                CtorInfo {
                    name: names.symbol(&constructor.name),
                    short_name: names.symbol(&constructor.short_name),
                    tycon,
                    fields: constructor.fields.clone(),
                    full_type: constructor.full_type.clone(),
                    tag: *next_tag,
                },
            );

            *next_tag += 1;
        }
    }

    fn data_def_named<'a>(tycon_table: &'a TyConTable, name: &str) -> &'a DataDefinition {
        let info = tycon_table
            .get(tycon_id(name))
            .expect("ICE: test fixture must define requested tycon");

        let TyConDefinition::Data(data_def) = &info.definition else {
            panic!("ICE: test fixture requested data definition from primitive tycon");
        };

        data_def
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
        Pattern::Con(ctor_id(name), args.into_iter().map(pattern_node).collect())
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

    fn unary_kind() -> Kind {
        Kind::make_arrow(Kind::Star, Kind::Star)
    }

    fn binary_kind() -> Kind {
        Kind::make_arrow(Kind::Star, unary_kind())
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
        TypeExpr::make_con_id(tycon_id(name))
    }

    fn tycon_id(name: &str) -> TyConId {
        match name {
            "Bool" => PRIM_TYS.bool,
            "Int" => PRIM_TYS.int,
            "Maybe" => TyConId(100),
            "List" => TyConId(101),
            "Pair" => TyConId(102),
            other => panic!("ICE: unknown test type constructor `{other}`"),
        }
    }

    fn ctor_id(name: &str) -> CtorId {
        match name {
            "True" => PRIM_TYS.true_ctor,
            "False" => PRIM_TYS.false_ctor,
            "Nothing" => CtorId(100),
            "Just" => CtorId(101),
            "Nil" => CtorId(102),
            "Cons" => CtorId(103),
            "MkPair" => CtorId(104),
            other => panic!("ICE: unknown test constructor `{other}`"),
        }
    }

    fn tyvar(name: &str) -> TypeExpr {
        TypeExpr::TyVar(test_tyvar_id(name))
    }

    fn app(func: TypeExpr, arg: TypeExpr) -> TypeExpr {
        TypeExpr::make_app(func, arg)
    }

    fn arr(arg: TypeExpr, result: TypeExpr) -> TypeExpr {
        TypeExpr::make_arr(arg, result)
    }

    fn forall(name: &str, body: TypeExpr) -> TypeExpr {
        TypeExpr::make_forall_id(test_tyvar_id(name), Kind::Star, body)
    }

    fn test_tyvar_id(name: &str) -> TyVarId {
        match name {
            "a" => TyVarId(0),
            "b" => TyVarId(1),
            other => panic!("ICE: unknown test type variable `{other}`"),
        }
    }
}
