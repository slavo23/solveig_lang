use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;

use crate::inference::exhaustiveness;
use crate::inference::holes::HoleInfo;
use crate::inference::inference_ctx::{Context, ContextEntry};
use solveig_ast::ast::{
    BinOp, CaseArm, CaseArmNode, ClassDefinition, ClassId, ClassMethodDef, CtorId, DefId, EvarId,
    InstanceDefinition, InstanceMethodDef, Kind, Literal, Pattern, PatternNode, Predicate, Pretty,
    ResolvedModule, Scheme, TermDefinition, TermExpr, TermExprNode, TyConId, TyVarId, TypeExpr,
    UnOp, VarRef, class_display_name, ctor_display_name, legacy_ctor_id, legacy_tyvar_id,
    legacy_tyvar_name, tyvar_display_name, var_display_name,
};
use solveig_base::utils::diagnostics::TypeError;
use solveig_base::{FileId, FileSpan, Span as SourceSpan};

include!("env.rs");
include!("elaborate.rs");
include!("constraints.rs");
include!("subtype.rs");
include!("pattern_check.rs");
include!("check.rs");
include!("infer.rs");
