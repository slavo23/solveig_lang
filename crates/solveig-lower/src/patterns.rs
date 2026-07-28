use solveig_ast::ast::Literal;
use std::rc::Rc;

/// Represents a lowered pattern shared by IR, closure IR, and bytecode match tables.
#[derive(Debug, Clone)]
pub enum LowerPattern {
    Var,
    Wild,
    Con(u16, Vec<LowerPattern>),
    Lit(Literal),
}

impl LowerPattern {
    /// Counts binders introduced when this pattern matches.
    pub fn binder_count(&self) -> usize {
        match self {
            LowerPattern::Var => 1,
            LowerPattern::Wild | LowerPattern::Lit(_) => 0,
            LowerPattern::Con(_, sub_patterns) => {
                sub_patterns.iter().map(|p| p.binder_count()).sum()
            }
        }
    }
}

/// Represents a lowered case arm whose body expression belongs to a pipeline stage.
#[derive(Debug, Clone)]
pub struct LowerCaseArm<E> {
    /// Matches the case scrutinee and introduces binders for the arm body.
    pub pattern: LowerPattern,
    /// Holds the body expression in the IR stage that owns this arm.
    pub body: Rc<E>,
}
