//! IDE-owned document link payloads for editor integrations.

use super::*;

/// Represents one source-level document link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDocumentLink {
    file_span: FileSpan,
    target: PathBuf,
    tooltip: Option<String>,
}

impl SourceDocumentLink {
    pub(super) fn new(file_span: FileSpan, target: impl Into<PathBuf>) -> Self {
        Self {
            file_span,
            target: target.into(),
            tooltip: None,
        }
    }

    pub(super) fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Returns the source range that should behave as a link.
    pub const fn file_span(&self) -> FileSpan {
        self.file_span
    }

    /// Returns the linked target path.
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Returns optional hover text for the link.
    pub fn tooltip(&self) -> Option<&str> {
        self.tooltip.as_deref()
    }
}
