use solveig_base::FxHashMap;
use std::path::PathBuf;
use std::rc::Rc;

use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};

use solveig_ast::ast::{
    BuiltinDefinition, ClassDefinition, ClassId, ClassTable, CtorId, CtorTable, DefId, Kind,
    ModuleId, Names, PRIM_TYS, Ty, TyConId, TyConTable,
};
use solveig_base::{Diagnostic, FileId, FileSpan, IntoDiagnostic, Label, Session, Span, Symbol};
use solveig_check::inference::type_infer::CheckedModule;
use solveig_syntax::ast::{
    Fixity, ImportDecl, ImportSpec, ModuleName, ParsedModule, SurfaceDefinition,
};

mod api_build;
mod graph;
mod imports;
mod module_api;
#[cfg(test)]
mod tests;

pub use api_build::*;
pub use graph::*;
pub use imports::*;
pub(crate) use module_api::local_def_id;
pub use module_api::*;
