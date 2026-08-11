use solveig_base::{FxHashMap, FxHashSet};
use std::ops::Range;

use solveig_ast::ast::{
    CaseArmNode, CtorId, CtorInfo, CtorTable, DataDefinition, Literal, MatchArmNode, Names,
    PRIM_TYS, Pattern, PatternNode, Predicate, PrettyTables, Ty, TyConDefinition, TyConId,
    TyConTable, TyVarId,
};
use solveig_base::TypeError;
use solveig_base::{FileId, FileSpan, Span as SourceSpan};

mod env;
mod matrix;
#[cfg(test)]
mod tests;
mod witness;

use env::{DataEnv, normalize_type};
use matrix::{
    ExhaustivenessChecker, lower_pattern, unreachable_match_warnings, unreachable_pattern_warnings,
};
use witness::{Pat, witness_to_pattern_row};

const MAX_MISSING_EXAMPLES: usize = 3;
type Span = Range<usize>;

/// Maranget-style exhaustiveness checking.
pub fn check_case(
    file: FileId,
    tycon_table: &TyConTable,
    ctor_table: &CtorTable,
    pretty_tables: PrettyTables<'_>,
    scrutinee_type: &Ty,
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

pub fn check_match(
    file: FileId,
    tycon_table: &TyConTable,
    ctor_table: &CtorTable,
    pretty_tables: PrettyTables<'_>,
    scrutinee_types: &[Ty],
    arms: &[MatchArmNode<Span>],
    match_span: Span,
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

    let scrutinee_types = scrutinee_types
        .iter()
        .map(normalize_type)
        .collect::<Vec<_>>();
    let matrix = arms
        .iter()
        .map(|arm| {
            arm.node
                .patterns
                .iter()
                .map(lower_pattern)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let vector = vec![Pat::Wild; scrutinee_types.len()];
    let warnings = unreachable_match_warnings(file, &checker, &matrix, &scrutinee_types, arms);
    let mut exhaustiveness_matrix = matrix;
    let mut missing = Vec::new();

    for _ in 0..MAX_MISSING_EXAMPLES {
        let Some(witness) = checker.useful(&exhaustiveness_matrix, &vector, &scrutinee_types)
        else {
            break;
        };

        let rendered = checker.pretty_witness_row(&witness);

        if missing.contains(&rendered) {
            break;
        }

        missing.push(rendered);
        exhaustiveness_matrix.push(witness_to_pattern_row(&witness));
    }

    let scrutinee_type = scrutinee_types
        .iter()
        .map(|ty| ty.pretty_with_tables(pretty_tables))
        .collect::<Vec<_>>()
        .join(", ");

    let error = if missing.is_empty() {
        None
    } else {
        Some(TypeError::NonExhaustivePatterns {
            span: FileSpan::new(file, SourceSpan::from_usize_range(match_span)),
            scrutinee_type,
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
