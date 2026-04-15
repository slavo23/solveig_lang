use core::ast::{BinOp, UnOp};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum CCExpr {
    Unit,
    Int(i64),
    Local(usize),
    Capture(usize),
    Global(usize),
    CtorFn {
        tag: u16,
        arity: u16,
    },
    UnaryOp(UnOp, Rc<CCExpr>),
    BinaryOp(BinOp, Rc<CCExpr>, Rc<CCExpr>),
    MakeClosure {
        func: usize,
        captures: Vec<Rc<CCExpr>>,
    },
    FunctionRef {
        func: usize,
        arity: usize,
    },
    Call {
        callee: Rc<CCExpr>,
        args: Vec<Rc<CCExpr>>,
    },
    CallKnown {
        func: usize,
        args: Vec<Rc<CCExpr>>,
    },
    Let(Rc<CCExpr>, Rc<CCExpr>),
    Pack {
        tag: u16,
        arity: u16,
        fields: Vec<Rc<CCExpr>>,
    },
    Case {
        scrutinee: Rc<CCExpr>,
        arms: Vec<CCCaseArm>,
    },
    IfThenElse {
        cond: Rc<CCExpr>,
        then_br: Rc<CCExpr>,
        else_br: Rc<CCExpr>,
    },
}

#[derive(Debug, Clone)]
pub struct CCCaseArm {
    pub pattern: CCPattern,
    pub body: Rc<CCExpr>,
}

#[derive(Debug, Clone)]
pub enum CCPattern {
    Var,
    Wild,
    Con(u16, Vec<CCPattern>),
}

impl CCPattern {
    pub fn binder_count(&self) -> usize {
        match self {
            CCPattern::Var => 1,
            CCPattern::Wild => 0,
            CCPattern::Con(_, sub_patterns) => sub_patterns.iter().map(|p| p.binder_count()).sum(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CCFunction {
    pub id: usize,
    pub name: String,
    pub arity: usize,
    pub capture_count: usize,
    pub body: Rc<CCExpr>,
}

#[derive(Debug, Clone)]
pub struct CCGlobalDef {
    pub name: String,
    pub body: Rc<CCExpr>,
}

#[derive(Debug, Clone)]
pub struct CCProgram {
    pub globals: Vec<CCGlobalDef>,
    pub functions: Vec<CCFunction>,
    pub constructor_arities: Vec<(String, u16, u16)>,
}

impl CCProgram {
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

    pub fn function(&self, id: usize) -> &CCFunction {
        &self.functions[id]
    }

    pub fn global(&self, id: usize) -> &CCGlobalDef {
        &self.globals[id]
    }
}
