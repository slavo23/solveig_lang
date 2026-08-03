//! IDE-owned declaration symbol payloads for editor navigation.

use super::*;

/// Classifies Solveig declarations for editor symbol requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceSymbolKind {
    /// A top-level function or operator.
    Function,
    /// An algebraic data type.
    Type,
    /// A type alias declaration.
    TypeAlias,
    /// A data constructor nested under its type.
    Constructor,
    /// A typeclass declaration.
    Class,
    /// A class method nested under its class.
    Method,
}

/// Represents one source declaration in an IDE artifact snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSymbol {
    name: String,
    kind: SourceSymbolKind,
    span: FileSpan,
    selection_span: FileSpan,
    container_name: Option<String>,
    children: Vec<SourceSymbol>,
}

impl SourceSymbol {
    pub(super) fn new(
        name: impl Into<String>,
        kind: SourceSymbolKind,
        span: FileSpan,
        selection_span: FileSpan,
        container_name: Option<String>,
        children: Vec<SourceSymbol>,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            span,
            selection_span,
            container_name,
            children,
        }
    }

    /// Returns the displayed declaration name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the declaration category.
    pub fn kind(&self) -> SourceSymbolKind {
        self.kind
    }

    /// Returns the enclosing source span for this declaration.
    pub fn file_span(&self) -> FileSpan {
        self.span
    }

    /// Returns the source span that selects this declaration's name.
    pub fn selection_span(&self) -> FileSpan {
        self.selection_span
    }

    /// Returns the module or parent declaration containing this symbol.
    pub fn container_name(&self) -> Option<&str> {
        self.container_name.as_deref()
    }

    /// Returns nested declaration symbols.
    pub fn children(&self) -> &[SourceSymbol] {
        &self.children
    }
}
