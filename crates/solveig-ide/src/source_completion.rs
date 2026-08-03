//! IDE-owned completion payloads for editor integrations.

use super::*;

/// Classifies Solveig completion items before LSP conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceCompletionItemKind {
    /// A local binder or variable.
    Variable,
    /// A top-level function or operator.
    Function,
    /// A data constructor.
    Constructor,
    /// A type constructor or type alias.
    Type,
    /// A typeclass.
    Class,
    /// A class method.
    Method,
    /// A module path segment.
    Module,
    /// A language keyword.
    Keyword,
}

/// Represents one source-level completion item after scope filtering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceCompletionItem {
    label: String,
    kind: SourceCompletionItemKind,
    detail: Option<String>,
    documentation: Option<String>,
}

impl SourceCompletionItem {
    pub(super) fn new(label: impl Into<String>, kind: SourceCompletionItemKind) -> Self {
        Self {
            label: label.into(),
            kind,
            detail: None,
            documentation: None,
        }
    }

    pub(super) fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub(super) fn with_optional_detail(mut self, detail: Option<String>) -> Self {
        self.detail = detail;
        self
    }

    pub(super) fn with_optional_documentation(mut self, documentation: Option<String>) -> Self {
        self.documentation = documentation;
        self
    }

    /// Returns the label displayed and inserted by editor clients.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the completion category.
    pub const fn kind(&self) -> SourceCompletionItemKind {
        self.kind
    }

    /// Returns the displayed type, kind, or short detail string.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// Returns documentation attached to the completion target, when available.
    pub fn documentation(&self) -> Option<&str> {
        self.documentation.as_deref()
    }
}

#[derive(Clone, Debug)]
pub(super) struct SourceCompletionCandidate {
    item: SourceCompletionItem,
    file: Option<FileId>,
    visible_span: Option<Span>,
    qualifier: Option<String>,
}

impl SourceCompletionCandidate {
    pub(super) fn new(item: SourceCompletionItem) -> Self {
        Self {
            item,
            file: None,
            visible_span: None,
            qualifier: None,
        }
    }

    pub(super) fn in_file(mut self, file: FileId) -> Self {
        self.file = Some(file);
        self
    }

    pub(super) fn in_scope(mut self, file: FileId, span: Span) -> Self {
        self.file = Some(file);
        self.visible_span = Some(span);
        self
    }

    pub(super) fn qualified(mut self, qualifier: impl Into<String>) -> Self {
        self.qualifier = Some(qualifier.into());
        self
    }

    pub(super) fn is_visible_at(&self, file: FileId, offset: u32, qualifier: Option<&str>) -> bool {
        if self
            .file
            .is_some_and(|candidate_file| candidate_file != file)
        {
            return false;
        }
        if self.qualifier.as_deref() != qualifier {
            return false;
        }

        self.visible_span.is_none_or(|span| {
            let offset = usize::try_from(offset).unwrap_or(usize::MAX);
            source_lookup::span_contains_offset(&span.to_usize_range(), offset)
        })
    }

    pub(super) fn item(&self) -> &SourceCompletionItem {
        &self.item
    }
}
