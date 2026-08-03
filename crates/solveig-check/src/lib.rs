//! Kind inference, type checking, evidence solving, and core validation.
//! The driver invokes this crate after name resolution and before lowering.

/// Validates checked core terms before lowering.
pub mod core_lint;
/// Infers kinds for algebraic data declarations.
pub mod data_kind_infer;
mod dependency_graph;
/// Contains bidirectional type checking, holes, and exhaustiveness analysis.
pub mod inference;
/// Provides the shared HM kind unifier used by kind inference passes.
pub mod kind_unify;
/// Infers kinds for type signatures and class predicates.
pub mod signature_infer;
