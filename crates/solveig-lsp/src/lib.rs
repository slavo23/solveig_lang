//! Language Server Protocol support for Solveig.
//! This crate adapts the driver and IDE APIs to `async-lsp` over stdio.

mod code_actions;
mod completion;
mod diagnostics;
mod editor_features;
mod hover;
mod inlay_hints;
mod navigation;
mod semantic_tokens;
mod server;
mod workspace;

pub use server::{LspServer, run_stdio, run_stdio_blocking};
