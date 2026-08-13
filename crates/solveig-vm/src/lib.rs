//! Solveig bytecode compiler and stack virtual machine runtime.
//! Lowered closure programs enter this crate for execution and IO handling.

/// Implements runtime primitive operations.
pub mod builtins;
/// Defines bytecode instructions and compiles closure IR into chunks.
pub mod bytecode;
/// Renders runtime values for output and tests.
pub mod display;
mod handle;
/// Owns heap allocation and garbage collection support.
pub mod heap;
/// Defines runtime values, frames, and executable chunks.
pub mod values;
/// Executes bytecode programs on the stack VM.
pub mod vm;
