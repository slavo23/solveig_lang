//! Closure-converted intermediate representation.
//! Bytecode generation consumes these terms after lambda lifting and capture analysis.

use solveig_ast::ast::{BinOp, BuiltinId, CtorRuntimeInfo, UnOp};
use solveig_base::FileSpan;
use std::rc::Rc;

use super::patterns::LowerCaseArm;

#[derive(Debug, Clone)]
pub enum CCExpr {
    Located(FileSpan, Rc<CCExpr>),
    Unit,
    Int(i64),
    Float(f64),
    Char(char),
    StrConst(u32),
    Local(usize),
    Capture(usize),
    Global(usize),
    CtorFn {
        tag: u16,
        arity: u16,
    },
    CallBuiltin {
        id: BuiltinId,
        args: Vec<Rc<CCExpr>>,
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
        arms: Vec<LowerCaseArm<CCExpr>>,
    },
    IfThenElse {
        cond: Rc<CCExpr>,
        then_br: Rc<CCExpr>,
        else_br: Rc<CCExpr>,
    },
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
    pub constructor_arities: Vec<CtorRuntimeInfo>,
    pub string_constants: Vec<String>,
    pub string_literals: Vec<String>,
}

impl CCProgram {
    pub fn constructor_name(&self, tag: u16) -> Option<String> {
        self.constructor_arities
            .iter()
            .find(|info| info.tag == tag)
            .map(|info| info.name.clone())
    }

    pub fn constructor_arity(&self, tag: u16) -> Option<u16> {
        self.constructor_arities
            .iter()
            .find(|info| info.tag == tag)
            .map(|info| info.arity)
    }

    pub fn function(&self, id: usize) -> &CCFunction {
        self.functions
            .get(id)
            .expect("ICE: closure function id not present in program")
    }

    pub fn global(&self, id: usize) -> &CCGlobalDef {
        self.globals
            .get(id)
            .expect("ICE: closure global id not present in program")
    }
}
