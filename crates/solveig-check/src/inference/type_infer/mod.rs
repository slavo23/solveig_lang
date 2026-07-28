use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;

use crate::inference::exhaustiveness;
use crate::inference::holes::HoleInfo;
use crate::inference::inference_ctx::{Context, ContextEntry};
use solveig_ast::ast::{
    CaseArm, CaseArmNode, ClassDefinition, ClassId, ClassMethodDef, ClassTable, CtorId, CtorTable,
    DefId, Ev, EvVarId, EvarId, InstanceDefinition, InstanceMethodDef, InstanceTable, Kind,
    Literal, Names, PRIM_TYS, Pattern, PatternNode, Predicate, PrettyTables, ResolvedModule,
    Scheme, TermDefinition, TermExpr, TermExprNode, Ty, TyConId, TyConTable, TyVarId, VarRef,
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
