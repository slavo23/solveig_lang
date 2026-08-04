//! Shared compiler infrastructure used across Solveig crates.
//!
//! This crate owns source locations, diagnostics, symbol interning, and the
//! preferred fast hash collections for compiler-internal lookup tables.

/// Reports an internal compiler error and aborts the current operation.
#[doc(hidden)]
#[expect(clippy::panic, reason = "ICE reporting is centralised here")]
pub fn report_ice(message: std::fmt::Arguments<'_>) -> ! {
    panic!("ICE: {message}")
}

/// Reports an internal compiler error at the single approved panic boundary.
#[macro_export]
macro_rules! ice {
    ($($arg:tt)*) => {
        $crate::report_ice(format_args!($($arg)*))
    };
}

/// Defines structured diagnostics emitted by compiler stages.
pub mod diagnostics;
/// Owns a compiler session and its source and symbol stores.
pub mod session;
/// Defines source files, file ids, and byte spans.
pub mod source;
/// Defines interned symbols and the shared string interner.
pub mod symbol;

/// Re-exports the shared diagnostic API used by frontend and checker crates.
pub use diagnostics::{Diagnostic, DiagnosticSink, IntoDiagnostic, Label, Severity, TypeError};
/// Re-exports the preferred hash collections for compiler-internal maps and sets.
pub use rustc_hash::{FxHashMap, FxHashSet};
/// Re-exports the compiler session type.
pub use session::Session;
/// Re-exports source-map, span, and editor position types.
pub use source::{FileId, FileSpan, PositionEncoding, SourceFile, SourceMap, SourcePosition, Span};
/// Re-exports symbol interning types.
pub use symbol::{Interner, Symbol};
