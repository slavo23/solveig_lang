//! Bidirectional type checking and evidence elaboration.
//! Included fragments share one ordered context following the checker pipeline.

use solveig_base::{FxHashMap, FxHashSet};
use std::collections::BTreeSet;
use std::ops::Range;

use crate::inference::exhaustiveness;
use crate::inference::holes::HoleInfo;
use crate::inference::inference_ctx::{Context, ContextEntry};
use solveig_ast::ast::{
    CaseArm, CaseArmNode, ClassDefinition, ClassId, ClassMethodDef, ClassTable, CtorId, CtorTable,
    DefId, Ev, EvVarId, EvarId, InstanceDefinition, InstanceId, InstanceMethodDef, Kind, Literal,
    MatchArm, MatchArmNode, NameScratch, Names, PRIM_TYS, Pattern, PatternNode, Predicate,
    PrettyTables, ResolvedModule, Scheme, TermDefinition, TermExpr, TermExprNode, Ty, TyConId,
    TyConTable, TyVarId, VarRef,
};
use solveig_base::TypeError;
use solveig_base::{FileId, FileSpan, Span as SourceSpan};

include!("env.rs");
include!("elaborate.rs");
include!("constraints.rs");
include!("subtype.rs");
include!("pattern_check.rs");
include!("check.rs");
include!("infer.rs");
