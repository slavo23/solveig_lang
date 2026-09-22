use solveig_ast::ast::{BinOp, BuiltinId, CtorRuntimeInfo, UnOp};
use solveig_base::FileSpan;
use std::rc::Rc;

use super::patterns::LowerCaseArm;

#[derive(Debug, Clone)]
pub enum IRExpr {
    Located(FileSpan, Rc<IRExpr>),
    Unit,
    BVar(usize),
    FVar(usize),
    Int(i64),
    BigIntConst(u32),
    Float(f64),
    Char(char),
    StrConst(u32),
    BinaryOp(BinOp, Rc<IRExpr>, Rc<IRExpr>),
    UnaryOp(UnOp, Rc<IRExpr>),
    CallBuiltin(BuiltinId, Vec<Rc<IRExpr>>),

    Abs(Rc<IRExpr>),
    App(Rc<IRExpr>, Rc<IRExpr>),
    Let(Rc<IRExpr>, Rc<IRExpr>),
    Con {
        tag: u16,
        arity: u16,
    },
    Case {
        scrutinee: Rc<IRExpr>,
        arms: Vec<LowerCaseArm<IRExpr>>,
    },
    IfThenElse {
        cond: Rc<IRExpr>,
        then_br: Rc<IRExpr>,
        else_br: Rc<IRExpr>,
    },
}

#[derive(Debug, Clone)]
pub struct IRProgram {
    pub globals: Vec<IRGlobalDef>,
    pub constructor_arities: Vec<CtorRuntimeInfo>,
    pub string_constants: Vec<String>,
    pub big_int_constants: Vec<String>,
    pub string_literals: Vec<String>,
}

impl IRProgram {
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
}

#[derive(Debug, Clone)]
pub struct IRGlobalDef {
    pub name: String,
    pub body: Rc<IRExpr>,
}
