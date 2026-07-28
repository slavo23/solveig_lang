//! Project graph construction and name resolution for Solveig modules.
//! This crate builds module APIs consumed by the checker and driver.

/// Builds project graphs and module APIs.
pub mod project;
/// Resolves names into semantic ids across modules.
pub mod resolver;
