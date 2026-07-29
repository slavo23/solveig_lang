//! Public driver API for parsing, resolving, checking, lowering, and running Solveig projects.
//! The CLI and tests use this crate as the orchestration boundary.

mod compile;
mod error;
mod project;
mod source_provider;

/// Re-exports project compilation, checking, and execution entry points.
pub use compile::{
    CheckResult, Compilation, CompileOptions, HoverInfo, ProjectDocs, ProjectSchemes,
    SourceCodeAction, SourceCompletionItem, SourceCompletionItemKind, SourceDocumentHighlight,
    SourceDocumentHighlightKind, SourceDocumentLink, SourceFoldingRange, SourceFoldingRangeKind,
    SourceInlayHint, SourceInlayHintCategory, SourceInlayHintConfig, SourceInlayHintKind,
    SourceSelectionRange, SourceSemanticToken, SourceSemanticTokenKind,
    SourceSemanticTokenModifier, SourceSemanticTokenModifiers, SourceSignatureHelp,
    SourceSignatureInformation, SourceSymbol, SourceSymbolKind, SourceTextEdit,
    checked_project_docs, checked_project_schemes, compile_and_run_project,
    compile_and_run_project_with_options, compile_and_run_project_with_options_and_args,
    run_project_capture, run_project_capture_with_stdin, run_project_capture_with_stdin_and_args,
};
/// Re-exports the top-level driver error type.
pub use error::DriverError;
