//! Language Server Protocol support for Solveig.
//! This crate adapts the driver API to `tower-lsp` over stdio.

mod diagnostics;
mod hover;
mod server;
mod workspace;

pub use server::{LspServer, run_stdio, run_stdio_blocking};
