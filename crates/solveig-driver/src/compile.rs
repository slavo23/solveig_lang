//! Driver compile facade.
//! Child modules own compilation state, frontend orchestration, builtins, Show wrappers, and evidence dumps.

use solveig_base::{FxHashMap, FxHashSet};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use solveig_ast::ast::{
    BUILTIN_ACCEPT, BUILTIN_APPEND_FILE, BUILTIN_BIND_IO, BUILTIN_BYTES_APPEND,
    BUILTIN_BYTES_INDEX, BUILTIN_BYTES_LENGTH, BUILTIN_BYTES_SLICE, BUILTIN_BYTES_TO_STRING,
    BUILTIN_CATCH_DEFECT_IO, BUILTIN_CATCH_IO, BUILTIN_CHAR_COMPARE, BUILTIN_CHAR_EQ,
    BUILTIN_CHAR_TO_STRING, BUILTIN_CHARS_TO_STRING, BUILTIN_CLOSE_CHAN, BUILTIN_CLOSE_SOCKET,
    BUILTIN_CONNECT, BUILTIN_ERROR, BUILTIN_FAIL_IO, BUILTIN_FINALLY_IO, BUILTIN_FLOAT_ADD,
    BUILTIN_FLOAT_DIV, BUILTIN_FLOAT_EQ, BUILTIN_FLOAT_GE, BUILTIN_FLOAT_GT, BUILTIN_FLOAT_LE,
    BUILTIN_FLOAT_LT, BUILTIN_FLOAT_MUL, BUILTIN_FLOAT_NEG, BUILTIN_FLOAT_SUB,
    BUILTIN_FLOAT_TO_INT, BUILTIN_FLOAT_TO_STRING, BUILTIN_GET_ARGS, BUILTIN_H_CLOSE,
    BUILTIN_H_FLUSH, BUILTIN_H_GET_BYTES, BUILTIN_H_GET_CONTENTS, BUILTIN_H_GET_LINE,
    BUILTIN_H_IS_EOF, BUILTIN_H_PUT_BYTES, BUILTIN_H_PUT_STR, BUILTIN_H_PUT_STR_LN,
    BUILTIN_H_SET_BUFFERING, BUILTIN_INT_ADD, BUILTIN_INT_COMPARE, BUILTIN_INT_DIV, BUILTIN_INT_EQ,
    BUILTIN_INT_GE, BUILTIN_INT_GT, BUILTIN_INT_LE, BUILTIN_INT_LT, BUILTIN_INT_MOD,
    BUILTIN_INT_MUL, BUILTIN_INT_NEG, BUILTIN_INT_SUB, BUILTIN_INT_TO_FLOAT, BUILTIN_INT_TO_STRING,
    BUILTIN_KILL_THREAD, BUILTIN_LISTEN, BUILTIN_MASK_IO, BUILTIN_NEW_BOUNDED_CHAN,
    BUILTIN_NEW_CHAN, BUILTIN_OPEN_FILE, BUILTIN_PRINT_LINE, BUILTIN_PRINT_STR, BUILTIN_PURE_IO,
    BUILTIN_RAISE_DEFECT_IO, BUILTIN_READ_FILE, BUILTIN_READ_FILE_BYTES, BUILTIN_READ_LINE,
    BUILTIN_RECV_BYTES, BUILTIN_RECV_CHAN, BUILTIN_RESOLVE_HOST, BUILTIN_RESTORE_MASK_IO,
    BUILTIN_SEND_BYTES, BUILTIN_SEND_CHAN, BUILTIN_SLEEP, BUILTIN_SPAWN, BUILTIN_STDERR,
    BUILTIN_STDIN, BUILTIN_STDOUT, BUILTIN_STRING_APPEND, BUILTIN_STRING_CHAR_AT,
    BUILTIN_STRING_COMPARE, BUILTIN_STRING_EQ, BUILTIN_STRING_LENGTH, BUILTIN_STRING_SLICE,
    BUILTIN_STRING_TO_BYTES, BUILTIN_STRING_TO_CHARS, BUILTIN_TRY_RECV_CHAN, BUILTIN_WRITE_FILE,
    BUILTIN_WRITE_FILE_BYTES, BuiltinDefinition, BuiltinId, COMPILER_PRELUDE_PRIM_MODULE,
    ClassDefinition, ClassId, ClassTable, CtorId, CtorTable, DefId, Ev, InstanceId, Kind, ModuleId,
    Names, PRIM_TYS, PrettyTables, Scheme, TermDefinition, TermExpr, TermExprNode, Ty, TyConId,
    TyConTable, TyVarId, VarRef, prelude_buffer_mode_tycon_id, prelude_defect_tycon_id,
    prelude_io_error_tycon_id, prelude_io_mode_tycon_id, prelude_list_tycon_id,
    prelude_maybe_tycon_id, prelude_ordering_tycon_id, prelude_pair_tycon_id,
    prelude_result_tycon_id,
};
use solveig_base::{Diagnostic, Severity, TypeError};
use solveig_base::{FileId, FileSpan, IntoDiagnostic, Session, SourceFile, SourceMap};
use solveig_check::core_lint::{CoreLintEnv, lint_checked_module};
use solveig_check::inference::holes::{HoleInfo, hole_diagnostics};
use solveig_check::inference::type_infer::{
    CheckedModule, TcMetadata, TypeCheckEnv, check_and_elab_expr_in_checked_module,
    check_and_elab_module,
};
use solveig_lower::closure_ir::CCProgram;
use solveig_lower::closure_lower::lower_to_closure_ir;
use solveig_lower::lowering::lower_module;
use solveig_resolve::project::{
    BuiltinApi, EffectiveImport, ImportedApi, ImportedTypedApi, ModuleContext, ModuleFile,
    ModuleTable, ParsedProject, Project, ResolveError, SharedModuleApi, SharedTypedModuleApi,
    TypeAlias, TypedDisplayMaps, build_module_api, build_typed_module_api, compiler_builtin_api,
    compiler_prelude_prim_module, effective_imports, module_order_checked,
    seed_compiler_prelude_module_api,
};
use solveig_resolve::resolver::{Resolver, ResolverError, ResolverImports};
use solveig_syntax::ast::{ImportSpec, ModuleName};
use solveig_vm::vm::Machine;

use crate::DriverError;
use crate::project::{
    ProjectError, is_compiler_prelude_module, load_project_with_source_provider, parse_project,
};
use crate::source_provider::SourceProvider;

mod builtins;
mod compilation;
mod evidence_dump;
mod incremental;
mod pipeline;
mod show_entry;

use builtins::driver_builtin_api;
pub use compilation::{
    CheckResult, Compilation, CompilationSnapshot, CompileOptions, ModuleResolveErrors,
    ProjectDocs, ProjectSchemes, checked_project_docs, checked_project_schemes,
    compile_and_run_project, compile_and_run_project_with_options,
    compile_and_run_project_with_options_and_args, run_project_capture,
    run_project_capture_with_stdin, run_project_capture_with_stdin_and_args,
};
use compilation::{CheckedProject, PreludeCache};
use evidence_dump::dump_solved_evidence;
use incremental::IncrementalCache;
use pipeline::{FrontendPhase, merge_checked_modules, verify_lowering_core};
use show_entry::{add_show_entry_wrapper, entry_returns_io};
