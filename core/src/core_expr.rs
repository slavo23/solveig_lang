use crate::core_type::{Ty, Kind};

#[derive(Clone, Debug)]
pub enum CoreExpr {
    Unit,
    Var(String),
    App(Box<CoreExpr>, Box<CoreExpr>),
    Abs(String, Box<CoreExpr>),
    Anno(Box<CoreExpr>, Ty),
    Let(String, Box<CoreExpr>, Box<CoreExpr>),
    Con(String),
    Fix(Box<CoreExpr>)
}

#[derive(Clone, Debug)]
pub enum CoreDefinition {
    FunDef {
        name: String,
        signature: Ty,
        body: CoreExpr
    }
}

#[derive(Clone, Debug)]
pub struct CoreConstructor {
    pub name: String,
    pub ty: Ty
}

#[derive(Clone, Debug)]
pub struct CoreDataDef {
    pub name: String,
    pub kind: Kind,
    pub ctors: Vec<CoreConstructor>
}

impl CoreExpr {
    pub fn app(lhs: CoreExpr, rhs: CoreExpr) -> CoreExpr {
        CoreExpr::App(Box::new(lhs), Box::new(rhs))
    }

    pub fn abs(arg: String, body: CoreExpr) -> CoreExpr {
        CoreExpr::Abs(arg, Box::new(body))
    }

    pub fn var(name: String) -> CoreExpr {
        CoreExpr::Var(name)
    }

    pub fn unit() -> CoreExpr {
        CoreExpr::Unit
    }

    pub fn anno(expr: CoreExpr, ty: Ty) -> CoreExpr {
        CoreExpr::Anno(Box::new(expr), ty)
    }

    pub fn let_expr(bind_name: String, body: CoreExpr, in_expr: CoreExpr) -> CoreExpr {
        CoreExpr::Let(bind_name.into(), Box::new(body), Box::new(in_expr))
    }

    pub fn fix(expr: CoreExpr) -> CoreExpr {
        CoreExpr::Fix(Box::new(expr))
    }
}
