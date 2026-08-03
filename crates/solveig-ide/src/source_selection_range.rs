//! IDE-owned selection range payloads for editor integrations.

use super::*;

/// Represents one source-level selection range and its containing parent range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSelectionRange {
    file_span: FileSpan,
    parent: Option<Box<SourceSelectionRange>>,
}

impl SourceSelectionRange {
    pub(super) fn new(file_span: FileSpan, parent: Option<SourceSelectionRange>) -> Self {
        Self {
            file_span,
            parent: parent.map(Box::new),
        }
    }

    /// Returns the source range suggested for selection.
    pub const fn file_span(&self) -> FileSpan {
        self.file_span
    }

    /// Returns the containing selection range, when one exists.
    pub fn parent(&self) -> Option<&SourceSelectionRange> {
        self.parent.as_deref()
    }
}
