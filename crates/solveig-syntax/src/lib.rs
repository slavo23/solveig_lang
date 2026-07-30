//! Solveig surface syntax definitions, lexer, parser, and fixity resolution.
//! The driver uses this crate as the frontend entry point before resolution.

/// Defines parsed surface syntax nodes.
pub mod ast;
/// Builds and navigates lossless concrete syntax trees.
pub mod cst;
/// Resolves infix fixity and unary minus precedence.
pub mod fixity;
/// Tokenizes Solveig source text into spanned tokens.
pub mod lexer;
/// Lowers parsed CSTs into surface syntax modules.
pub mod lowering;
/// Parses tokens into lossless CSTs.
pub mod parser;
/// Defines parser events and concrete syntax node kinds.
pub mod syntax;
