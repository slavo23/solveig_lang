//! Kind inference, type checking, evidence solving, and core validation.
//! The driver invokes this crate after name resolution and before lowering.

/// Validates checked core terms before lowering.
pub mod core_lint;
/// Infers kinds for algebraic data declarations.
pub mod data_kind_infer;
/// Contains bidirectional type checking, holes, and exhaustiveness analysis.
pub mod inference;
/// Infers kinds for type signatures and class predicates.
pub mod signature_infer;
