#[derive(Clone, Debug)]
pub enum CoreExpr {
    Unit,
    Var(String),
    App(Box<CoreExpr>, Box<CoreExpr>),
    Abs(String, Box<CoreExpr>),
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
}