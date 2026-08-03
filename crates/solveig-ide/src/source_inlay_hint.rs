//! IDE-owned inlay hint payloads for editor integrations.

use super::*;

/// Classifies Solveig inlay hints before LSP conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceInlayHintKind {
    /// A type hint rendered inline in source.
    Type,
}

/// Identifies the user-configurable inlay hint category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceInlayHintCategory {
    /// Inferred type for an unannotated `let` binding.
    LetBindingType,
    /// Inferred type for a top-level function parameter.
    FunctionParameterType,
    /// Inferred type for an unannotated lambda parameter.
    LambdaParameterType,
    /// Inferred full signature for a definition without a signature.
    InferredSignature,
}

/// Enables or disables groups of source-level inlay hints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceInlayHintConfig {
    pub let_binding_types: bool,
    pub function_parameter_types: bool,
    pub lambda_parameter_types: bool,
    pub inferred_signatures: bool,
}

impl Default for SourceInlayHintConfig {
    fn default() -> Self {
        Self {
            let_binding_types: true,
            function_parameter_types: true,
            lambda_parameter_types: true,
            inferred_signatures: true,
        }
    }
}

impl SourceInlayHintConfig {
    /// Returns whether hints in `category` are enabled.
    pub const fn enables(self, category: SourceInlayHintCategory) -> bool {
        match category {
            SourceInlayHintCategory::LetBindingType => self.let_binding_types,
            SourceInlayHintCategory::FunctionParameterType => self.function_parameter_types,
            SourceInlayHintCategory::LambdaParameterType => self.lambda_parameter_types,
            SourceInlayHintCategory::InferredSignature => self.inferred_signatures,
        }
    }
}

/// Represents one source-level inlay hint after range and setting filtering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceInlayHint {
    file_span: FileSpan,
    label: String,
    kind: SourceInlayHintKind,
    category: SourceInlayHintCategory,
}

impl SourceInlayHint {
    pub(super) fn new(
        file_span: FileSpan,
        label: impl Into<String>,
        kind: SourceInlayHintKind,
        category: SourceInlayHintCategory,
    ) -> Self {
        Self {
            file_span,
            label: label.into(),
            kind,
            category,
        }
    }

    /// Returns the zero-width source span where the hint should render.
    pub const fn file_span(&self) -> FileSpan {
        self.file_span
    }

    /// Returns the text displayed by editor clients.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the inlay hint kind.
    pub const fn kind(&self) -> SourceInlayHintKind {
        self.kind
    }

    /// Returns the settings category that controls this hint.
    pub const fn category(&self) -> SourceInlayHintCategory {
        self.category
    }
}
