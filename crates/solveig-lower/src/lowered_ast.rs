use solveig_ast::ast::{BinOp, Literal, UnOp};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum IRExpr {
    Unit,
    BVar(usize),
    FVar(usize),
    Int(i64),
    BinaryOp(BinOp, Rc<IRExpr>, Rc<IRExpr>),
    UnaryOp(UnOp, Rc<IRExpr>),

    Abs(Rc<IRExpr>),
    App(Rc<IRExpr>, Rc<IRExpr>),
    Let(Rc<IRExpr>, Rc<IRExpr>),
    Con {
        tag: u16,
        arity: u16,
    },
    Case {
        scrutinee: Rc<IRExpr>,
        arms: Vec<IRCaseArm>,
    },
    IfThenElse {
        cond: Rc<IRExpr>,
        then_br: Rc<IRExpr>,
        else_br: Rc<IRExpr>,
    },
}

#[derive(Debug, Clone)]
pub struct IRCaseArm {
    pub pattern: IRPattern,
    pub body: Rc<IRExpr>,
}

#[derive(Debug, Clone)]
pub enum IRPattern {
    Var,
    Wild,
    Con(u16, Vec<IRPattern>),
    Lit(Literal),
}

impl IRPattern {
    pub fn binder_count(&self) -> usize {
        match self {
            IRPattern::Var => 1,
            IRPattern::Wild => 0,
            IRPattern::Lit(_) => 0,
            IRPattern::Con(_, sub_patterns) => sub_patterns.iter().map(|p| p.binder_count()).sum(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IRProgram {
    pub globals: Vec<IRGlobalDef>,
    pub constructor_arities: Vec<(String, u16, u16)>,
}

impl IRProgram {
    pub fn constructor_name(&self, tag: u16) -> Option<&str> {
        self.constructor_arities
            .iter()
            .find(|(_, ctor_tag, _)| *ctor_tag == tag)
            .map(|(name, _, _)| name.as_str())
    }

    pub fn constructor_tag(&self, name: &str) -> Option<u16> {
        self.constructor_arities
            .iter()
            .find(|(ctor_name, _, _)| ctor_name == name)
            .map(|(_, tag, _)| *tag)
    }

    pub fn constructor_arity(&self, tag: u16) -> Option<u16> {
        self.constructor_arities
            .iter()
            .find(|(_, ctor_tag, _)| *ctor_tag == tag)
            .map(|(_, _, arity)| *arity)
    }
}

#[derive(Debug, Clone)]
pub struct IRGlobalDef {
    pub name: String,
    pub body: Rc<IRExpr>,
}
