//! Driver-owned document highlight payloads for editor integrations.

use super::*;

/// Classifies a source document highlight before LSP conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceDocumentHighlightKind {
    /// A readable reference to a symbol.
    Read,
    /// A binding or declaration site for a symbol.
    Write,
}

/// Represents one source-level document highlight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDocumentHighlight {
    file_span: FileSpan,
    kind: SourceDocumentHighlightKind,
}

impl SourceDocumentHighlight {
    pub(super) fn new(file_span: FileSpan, kind: SourceDocumentHighlightKind) -> Self {
        Self { file_span, kind }
    }

    /// Returns the source range highlighted by editor clients.
    pub const fn file_span(&self) -> FileSpan {
        self.file_span
    }

    /// Returns the highlight category.
    pub const fn kind(&self) -> SourceDocumentHighlightKind {
        self.kind
    }
}
