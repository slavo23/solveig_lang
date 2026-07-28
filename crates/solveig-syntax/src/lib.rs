//! Solveig surface syntax definitions, lexer, parser, and fixity resolution.
//! The driver uses this crate as the frontend entry point before resolution.

/// Defines parsed surface syntax nodes.
pub mod ast;
/// Resolves infix fixity and unary minus precedence.
pub mod fixity;
/// Tokenizes Solveig source text into spanned tokens.
pub mod lexer;
/// Parses tokens into surface syntax modules.
pub mod parser;
/// Defines parser events and concrete syntax node kinds.
pub mod syntax;
