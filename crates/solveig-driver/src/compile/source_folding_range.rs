//! Driver-owned folding range payloads for editor integrations.

use super::*;

/// Classifies source folding ranges before LSP conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceFoldingRangeKind {
    /// A syntax region such as a declaration or expression body.
    Region,
    /// A comment block or consecutive line-comment region.
    Comment,
}

/// Represents one foldable source range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFoldingRange {
    file_span: FileSpan,
    kind: SourceFoldingRangeKind,
}

impl SourceFoldingRange {
    pub(super) fn new(file_span: FileSpan, kind: SourceFoldingRangeKind) -> Self {
        Self { file_span, kind }
    }

    /// Returns the source range covered by this fold.
    pub const fn file_span(&self) -> FileSpan {
        self.file_span
    }

    /// Returns the fold category.
    pub const fn kind(&self) -> SourceFoldingRangeKind {
        self.kind
    }
}
