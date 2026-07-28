//! Core AST facade re-exporting the concept modules used across Solveig.
//! Downstream crates import through `solveig_ast::ast` while this directory owns the split implementation.

const PRECEDENCE_LOOSE: u8 = 0;
const PRECEDENCE_ARR: u8 = 1;
const PRECEDENCE_APP: u8 = 2;
const PRECEDENCE_ATOM: u8 = 3;

pub trait Pretty<T> {
    fn pretty(&self) -> String;
    fn get_precedence(t: &T) -> u8;
}

mod ids;
mod instances;
mod kind;
mod names;
mod pretty;
mod scheme;
mod tables;
mod term;
mod ty;

pub use ids::*;
pub use instances::*;
pub use kind::*;
pub use names::*;
pub use pretty::*;
pub use scheme::*;
pub use tables::*;
pub use term::*;
pub use ty::*;

#[cfg(test)]
mod tests;
