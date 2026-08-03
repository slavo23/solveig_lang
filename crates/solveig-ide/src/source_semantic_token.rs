//! IDE-owned semantic-token DTOs for editor integrations.

use super::*;

/// Classifies source spans for editor semantic highlighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceSemanticTokenKind {
    /// A module path in an import.
    Namespace,
    /// A data type or type alias.
    Type,
    /// A typeclass.
    Class,
    /// A type parameter binder or use.
    TypeParameter,
    /// A function, lambda, case, or do parameter binder.
    Parameter,
    /// A local variable or unresolved lower-case identifier.
    Variable,
    /// A top-level function or operator.
    Function,
    /// A data constructor.
    EnumMember,
    /// A class method.
    Method,
    /// A Solveig keyword.
    Keyword,
    /// A symbolic operator.
    Operator,
    /// A string or character literal.
    String,
    /// An integer or float literal.
    Number,
    /// A line or block comment.
    Comment,
}

/// Classifies semantic-token decorations used by editors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceSemanticTokenModifier {
    /// Marks declarations that introduce a name.
    Declaration,
    /// Marks definitions that provide an implementation or constructor.
    Definition,
    /// Marks immutable values from Solveig source.
    Readonly,
    /// Marks symbols that come from the compiler prelude.
    DefaultLibrary,
}

/// Stores the modifier set for one semantic token.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceSemanticTokenModifiers(u8);

impl SourceSemanticTokenModifiers {
    /// Creates an empty modifier set.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Creates a modifier set containing only `modifier`.
    pub const fn one(modifier: SourceSemanticTokenModifier) -> Self {
        Self(modifier_bit(modifier))
    }

    /// Returns a copy with `modifier` added.
    pub const fn with(self, modifier: SourceSemanticTokenModifier) -> Self {
        Self(self.0 | modifier_bit(modifier))
    }

    /// Reports whether `modifier` is present.
    pub const fn contains(self, modifier: SourceSemanticTokenModifier) -> bool {
        self.0 & modifier_bit(modifier) != 0
    }

    /// Returns modifiers in stable legend order.
    pub fn iter(self) -> impl Iterator<Item = SourceSemanticTokenModifier> {
        [
            SourceSemanticTokenModifier::Declaration,
            SourceSemanticTokenModifier::Definition,
            SourceSemanticTokenModifier::Readonly,
            SourceSemanticTokenModifier::DefaultLibrary,
        ]
        .into_iter()
        .filter(move |modifier| self.contains(*modifier))
    }
}

/// Represents one classified source span for editor semantic highlighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceSemanticToken {
    span: FileSpan,
    kind: SourceSemanticTokenKind,
    modifiers: SourceSemanticTokenModifiers,
}

impl SourceSemanticToken {
    /// Creates one classified source span.
    pub const fn new(
        span: FileSpan,
        kind: SourceSemanticTokenKind,
        modifiers: SourceSemanticTokenModifiers,
    ) -> Self {
        Self {
            span,
            kind,
            modifiers,
        }
    }

    /// Returns the source span covered by this token.
    pub const fn file_span(self) -> FileSpan {
        self.span
    }

    /// Returns the semantic token kind.
    pub const fn kind(self) -> SourceSemanticTokenKind {
        self.kind
    }

    /// Returns this token's modifiers.
    pub const fn modifiers(self) -> SourceSemanticTokenModifiers {
        self.modifiers
    }
}

const fn modifier_bit(modifier: SourceSemanticTokenModifier) -> u8 {
    match modifier {
        SourceSemanticTokenModifier::Declaration => 1 << 0,
        SourceSemanticTokenModifier::Definition => 1 << 1,
        SourceSemanticTokenModifier::Readonly => 1 << 2,
        SourceSemanticTokenModifier::DefaultLibrary => 1 << 3,
    }
}
