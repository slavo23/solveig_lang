//! Pattern-matrix usefulness and coverage logic.
//! The type checker uses this module for unreachable-arm and missing-pattern diagnostics.

use super::*;
use crate::inference::exhaustiveness::env::normalize_type;
use crate::inference::exhaustiveness::witness::{Head, LitPat, Pat, Witness};

pub(super) struct ExhaustivenessChecker<'a> {
    pub(super) data_env: DataEnv<'a>,
    pub(super) ctor_table: &'a CtorTable,
}

impl ExhaustivenessChecker<'_> {
    pub(super) fn pretty_witness_row(&self, witness: &[Witness]) -> String {
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

    // Maranget 2007, "Warnings for pattern matching"; this is the usefulness
    // judgment over a pattern matrix, with witnesses kept for diagnostics.
    pub(super) fn useful(
        &self,
        matrix: &[Vec<Pat>],
        vector: &[Pat],
        types: &[Ty],
    ) -> Option<Vec<Witness>> {
        if matrix.is_empty() {
            return Some(vec![Witness::Wild; vector.len()]);
        }

        if vector.is_empty() {
            return None;
        }

        let (first_pat, vector_tail) = vector.split_first()?;
        let (first_type_raw, type_tail) = types.split_first()?;
        let first_type = normalize_type(first_type_raw);

        match first_pat {
            Pat::Head(head, args) => {
                let field_types = self.field_types_for_head(head, &first_type)?;
                let field_count = field_types.len();

                let specialized_matrix = self.specialize_matrix(matrix, head, field_count);

                let mut specialized_vector = args.clone();
                specialized_vector.extend_from_slice(vector_tail);

                let mut specialized_types = field_types;
                specialized_types.extend_from_slice(type_tail);

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
        types: &[Ty],
        first_type: &Ty,
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
            self.useful_wild_complete_signature(matrix, vector, types, first_type, &constructors)
        } else {
            self.useful_wild_incomplete_signature(
                matrix,
                vector,
                types,
                first_type,
                &constructors,
                &present_constructors,
            )
        }
    }

    fn useful_wild_complete_signature(
        &self,
        matrix: &[Vec<Pat>],
        vector: &[Pat],
        types: &[Ty],
        first_type: &Ty,
        constructors: &[CtorId],
    ) -> Option<Vec<Witness>> {
        for constructor_id in constructors.iter().copied() {
            let head = Head::Constructor(constructor_id);

            let Some(field_types) = self.field_types_for_head(&head, first_type) else {
                continue;
            };

            let field_count = field_types.len();
            let specialized_matrix = self.specialize_matrix(matrix, &head, field_count);

            let mut specialized_vector = vec![Pat::Wild; field_count];
            specialized_vector.extend_from_slice(vector.get(1..)?);

            let mut specialized_types = field_types;
            specialized_types.extend_from_slice(types.get(1..)?);

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
        types: &[Ty],
        first_type: &Ty,
        constructors: &[CtorId],
        present_constructors: &FxHashSet<CtorId>,
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
            .first_missing_constructor(constructors, present_constructors)
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
        types: &[Ty],
    ) -> Option<Vec<Witness>> {
        debug_assert!(!vector.is_empty());

        let default_matrix = self.default_matrix(matrix);

        self.useful(&default_matrix, vector.get(1..)?, types.get(1..)?)
    }

    fn present_constructors(&self, matrix: &[Vec<Pat>]) -> FxHashSet<CtorId> {
        let mut present = FxHashSet::default();

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
        present_constructors: &FxHashSet<CtorId>,
    ) -> Option<CtorId> {
        constructors
            .iter()
            .find(|constructor| !present_constructors.contains(*constructor))
            .copied()
    }

    fn field_types_for_head(&self, head: &Head, scrutinee_type: &Ty) -> Option<Vec<Ty>> {
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

pub(super) fn lower_pattern(pattern: &PatternNode<Span>) -> Pat {
    match &pattern.node {
        Pattern::Wild | Pattern::Var(_) => Pat::Wild,

        Pattern::Lit(lit) => Pat::Head(Head::Literal(LitPat::from_literal(lit)), Vec::new()),

        Pattern::Con(id, args) => Pat::Head(
            Head::Constructor(*id),
            args.iter().map(lower_pattern).collect(),
        ),
    }
}

pub(super) fn unreachable_pattern_warnings(
    file: FileId,
    checker: &ExhaustivenessChecker<'_>,
    matrix: &[Vec<Pat>],
    types: &[Ty],
    arms: &[CaseArmNode<Span>],
) -> Vec<TypeError> {
    let mut warnings = Vec::new();

    for (index, row) in matrix.iter().enumerate() {
        if checker
            .useful(
                matrix
                    .get(..index)
                    .expect("ICE: unreachable pattern matrix prefix out of bounds"),
                row,
                types,
            )
            .is_none()
        {
            let arm = arms
                .get(index)
                .expect("ICE: pattern matrix row without corresponding case arm");
            warnings.push(TypeError::UnreachablePattern {
                span: FileSpan::new(
                    file,
                    SourceSpan::from_usize_range(arm.node.pattern.anno.clone()),
                ),
            });
        }
    }

    warnings
}
