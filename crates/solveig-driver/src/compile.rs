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
    BUILTIN_BYTES_CONCAT, BUILTIN_BYTES_EQ, BUILTIN_BYTES_FIND_ANY, BUILTIN_BYTES_FIND_BELOW,
    BUILTIN_BYTES_INDEX, BUILTIN_BYTES_LENGTH, BUILTIN_BYTES_SLICE, BUILTIN_BYTES_SLICE_TO_STRING,
    BUILTIN_BYTES_SPAN_ANY, BUILTIN_BYTES_SPAN_RANGE, BUILTIN_BYTES_TO_STRING,
    BUILTIN_BYTES_TO_STRING_REPLACING, BUILTIN_BYTES_UTF8_VALID_PREFIX, BUILTIN_CATCH_DEFECT_IO,
    BUILTIN_CATCH_IO, BUILTIN_CHAR_COMPARE, BUILTIN_CHAR_EQ, BUILTIN_CHAR_TO_CODE_POINT,
    BUILTIN_CHAR_TO_STRING, BUILTIN_CHARS_TO_STRING, BUILTIN_CLOSE_CHAN, BUILTIN_CLOSE_SOCKET,
    BUILTIN_CODE_POINT_TO_STRING, BUILTIN_CONNECT, BUILTIN_CREATE_DIRECTORY,
    BUILTIN_DOES_DIRECTORY_EXIST, BUILTIN_DOES_FILE_EXIST, BUILTIN_ERROR, BUILTIN_EXIT_WITH,
    BUILTIN_FAIL_IO, BUILTIN_FINALLY_IO, BUILTIN_FLOAT_ADD, BUILTIN_FLOAT_DIV, BUILTIN_FLOAT_EQ,
    BUILTIN_FLOAT_GE, BUILTIN_FLOAT_GT, BUILTIN_FLOAT_LE, BUILTIN_FLOAT_LT, BUILTIN_FLOAT_MUL,
    BUILTIN_FLOAT_NEG, BUILTIN_FLOAT_SUB, BUILTIN_FLOAT_TO_INT, BUILTIN_FLOAT_TO_STRING,
    BUILTIN_FLOAT_TOTAL_COMPARE, BUILTIN_GET_ARGS, BUILTIN_GET_CURRENT_DIRECTORY, BUILTIN_GET_ENV,
    BUILTIN_GET_ENVIRONMENT, BUILTIN_GET_FILE_SIZE, BUILTIN_GET_MODIFICATION_TIME,
    BUILTIN_GET_MONOTONIC_TIME, BUILTIN_GET_PROG_NAME, BUILTIN_GET_SYSTEM_TIME, BUILTIN_H_CLOSE,
    BUILTIN_H_FLUSH, BUILTIN_H_GET_BYTES, BUILTIN_H_GET_CONTENTS, BUILTIN_H_GET_LINE,
    BUILTIN_H_IS_EOF, BUILTIN_H_PUT_BYTES, BUILTIN_H_PUT_STR, BUILTIN_H_PUT_STR_LN,
    BUILTIN_H_SET_BUFFERING, BUILTIN_HTTP_REQUEST_RAW, BUILTIN_INT_ADD, BUILTIN_INT_COMPARE,
    BUILTIN_INT_DIV, BUILTIN_INT_EQ, BUILTIN_INT_GE, BUILTIN_INT_GT, BUILTIN_INT_LE,
    BUILTIN_INT_LT, BUILTIN_INT_MOD, BUILTIN_INT_MUL, BUILTIN_INT_NEG, BUILTIN_INT_SUB,
    BUILTIN_INT_TO_FLOAT, BUILTIN_INT_TO_STRING, BUILTIN_KILL_THREAD, BUILTIN_LIST_DIRECTORY,
    BUILTIN_LISTEN, BUILTIN_MASK_IO, BUILTIN_MK_STD_GEN, BUILTIN_NEW_BOUNDED_CHAN,
    BUILTIN_NEW_CHAN, BUILTIN_NEW_STD_GEN, BUILTIN_OPEN_FILE, BUILTIN_PATH_EXTENSION,
    BUILTIN_PATH_FILE_NAME, BUILTIN_PATH_IS_ABSOLUTE, BUILTIN_PATH_JOIN, BUILTIN_PATH_NORMALISE,
    BUILTIN_PATH_PARENT, BUILTIN_PRINT_LINE, BUILTIN_PRINT_STR, BUILTIN_PURE_IO,
    BUILTIN_RAISE_DEFECT_IO, BUILTIN_RANDOM_INT, BUILTIN_RANDOM_RANGE, BUILTIN_READ_FILE,
    BUILTIN_READ_FILE_BYTES, BUILTIN_READ_FILE_BYTES_LIMITED, BUILTIN_READ_FILE_LIMITED,
    BUILTIN_READ_LINE, BUILTIN_RECV_BYTES, BUILTIN_RECV_CHAN, BUILTIN_REMOVE_FILE,
    BUILTIN_RENAME_FILE, BUILTIN_RESOLVE_HOST, BUILTIN_RESTORE_MASK_IO,
    BUILTIN_RUN_PROCESS_CONFIGURED, BUILTIN_SECURE_RANDOM_BYTES, BUILTIN_SEND_BYTES,
    BUILTIN_SEND_CHAN, BUILTIN_SET_CURRENT_DIRECTORY, BUILTIN_SET_ENV, BUILTIN_SLEEP,
    BUILTIN_SPAWN, BUILTIN_STDERR, BUILTIN_STDIN, BUILTIN_STDOUT, BUILTIN_STRING_APPEND,
    BUILTIN_STRING_BYTE_LENGTH, BUILTIN_STRING_BYTE_SLICE, BUILTIN_STRING_CHAR_AT,
    BUILTIN_STRING_CHAR_AT_BYTE, BUILTIN_STRING_COMPARE, BUILTIN_STRING_EQ,
    BUILTIN_STRING_FIND_CHAR_FROM, BUILTIN_STRING_LENGTH, BUILTIN_STRING_SLICE,
    BUILTIN_STRING_STARTS_WITH, BUILTIN_STRING_TO_BYTES, BUILTIN_STRING_TO_CHARS,
    BUILTIN_STRING_TRIM_ASCII, BUILTIN_STRINGS_CONCAT, BUILTIN_TRY_RECV_CHAN, BUILTIN_WRITE_FILE,
    BUILTIN_WRITE_FILE_ATOMIC, BUILTIN_WRITE_FILE_BYTES, BUILTIN_WRITE_FILE_BYTES_ATOMIC,
    BuiltinDefinition, BuiltinId, COMPILER_PRELUDE_PRIM_MODULE, ClassDefinition, ClassId,
    ClassTable, CtorId, CtorTable, DefId, Ev, InstanceId, Kind, ModuleId, Names, PRIM_TYS,
    PrettyTables, Scheme, TermDefinition, TermExpr, TermExprNode, Ty, TyConId, TyConTable, TyVarId,
    VarRef, prelude_defect_tycon_id, prelude_either_tycon_id, prelude_host_error_tycon_id,
    prelude_list_tycon_id, prelude_maybe_tycon_id, prelude_ordering_tycon_id,
    prelude_tuple_tycon_id, prelude_utf8_error_tycon_id, std_buffer_mode_tycon_id,
    std_io_mode_tycon_id, std_process_result_tycon_id,
};
use solveig_base::{Diagnostic, Severity, TypeError};
use solveig_base::{FileId, FileSpan, IntoDiagnostic, Session, SourceFile, SourceMap, Symbol};
use solveig_check::core_lint::{CoreLintEnv, lint_checked_module};
use solveig_check::inference::holes::{HoleInfo, hole_diagnostics};
use solveig_check::inference::type_infer::{
    CheckedModule, TcMetadata, TypeCheckEnv, check_and_elab_expr_in_checked_module,
    check_and_elab_module,
};
use solveig_lower::closure_ir::CCProgram;
use solveig_lower::closure_lower::lower_to_closure_ir;
use solveig_lower::lowering::lower_module;
use solveig_package::{Manifest, PackageId, find_nearest_manifest};
use solveig_resolve::project::{
    BuiltinApi, EffectiveImport, ImportedApi, ImportedTypedApi, ModuleApi, ModuleContext,
    ModuleTable, ParsedProject, Project, ResolveError, SharedModuleApi, SharedTypedModuleApi,
    TypeAlias, TypedDisplayMaps, apply_module_reexports, apply_module_visibility, build_module_api,
    build_typed_module_api, compiler_builtin_api, compiler_prelude_prim_module, effective_imports,
    module_order_checked, project_module_selection_diagnostics, seed_compiler_prelude_module_api,
    validate_instance_coherence,
};
use solveig_resolve::resolver::{Resolver, ResolverError, ResolverImports};
use solveig_syntax::ast::{ModuleName, ModuleSelection, ParsedModule};
use solveig_vm::vm::{ExitStatus, Machine};

use crate::DriverError;
use crate::project::{ProjectError, load_project_with_source_provider, parse_project};
use crate::source_provider::SourceProvider;

mod builtins;
mod compilation;
mod evidence_dump;
mod incremental;
mod pipeline;
mod show_entry;

use builtins::driver_builtin_api;
use compilation::CheckedProject;
pub use compilation::{
    CheckResult, Compilation, CompilationSnapshot, CompileOptions, ModuleResolveErrors,
    ProjectDocs, ProjectSchemes, checked_project_docs, checked_project_schemes,
    compile_and_run_project, compile_and_run_project_with_options,
    compile_and_run_project_with_options_and_args, run_project_capture,
    run_project_capture_with_stdin, run_project_capture_with_stdin_and_args,
};
use evidence_dump::dump_solved_evidence;
use incremental::{CachedPackage, PackageCache};
use pipeline::{FrontendPhase, merge_checked_modules, verify_lowering_core};
use show_entry::{add_show_entry_wrapper, entry_returns_io};
