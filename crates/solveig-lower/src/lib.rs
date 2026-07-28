//! Lowering passes from checked Solveig terms to closure-ready intermediate forms.
//! This crate sits between type checking and bytecode compilation.

/// Defines closure-converted IR consumed by bytecode generation.
pub mod closure_ir;
/// Converts lowered terms into closure IR and capture layouts.
pub mod closure_lower;
/// Plans runtime dictionary layouts for typeclass evidence.
pub mod dicts;
/// Defines lowered term forms used before closure conversion.
pub mod lowered_ast;
/// Lowers checked terms into VM-oriented core syntax.
pub mod lowering;
/// Defines lowered pattern forms shared by lowering and bytecode compilation.
pub mod patterns;

/// Re-exports lowered pattern types used by the VM compiler.
pub use patterns::{LowerCaseArm, LowerPattern};
