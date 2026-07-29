//! Driver-owned quick-fix payloads for editor code actions.

use super::*;

/// Represents one textual edit in a source file before protocol conversion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceTextEdit {
    file: FileId,
    span: Span,
    new_text: String,
}

impl SourceTextEdit {
    pub(super) fn new(file: FileId, span: Span, new_text: impl Into<String>) -> Self {
        Self {
            file,
            span,
            new_text: new_text.into(),
        }
    }

    /// Returns the source file changed by this edit.
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// Returns the byte span replaced by this edit.
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Returns this edit span with its source file id attached.
    pub fn file_span(&self) -> FileSpan {
        FileSpan::new(self.file, self.span)
    }

    /// Returns the replacement text.
    pub fn new_text(&self) -> &str {
        &self.new_text
    }
}

/// Represents a source-level quick fix retained by the driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceCodeAction {
    title: String,
    trigger: FileSpan,
    edits: Vec<SourceTextEdit>,
}

impl SourceCodeAction {
    pub(super) fn new(
        title: impl Into<String>,
        trigger: FileSpan,
        edits: Vec<SourceTextEdit>,
    ) -> Self {
        Self {
            title: title.into(),
            trigger,
            edits,
        }
    }

    /// Returns the title displayed by editor quick-fix menus.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the diagnostic or declaration span that should reveal this action.
    pub const fn trigger_span(&self) -> FileSpan {
        self.trigger
    }

    /// Returns the source edits applied by this action.
    pub fn edits(&self) -> &[SourceTextEdit] {
        &self.edits
    }
}
