#[derive(Clone, PartialEq, Debug)]
pub enum Ty {
    TyUnit,
    TyVar(String),
    TyExVar(String),
    TyArr(Box<Ty>, Box<Ty>),
    TyForAll(String, Box<Ty>),
}

impl Ty {
    pub fn pretty(&self) -> String {
        match self {
            Ty::TyUnit => "()".into(),
            Ty::TyVar(name) | Ty::TyExVar(name) => name.clone(),
            Ty::TyArr(lhs, rhs) => format!("{} -> {}", lhs.pretty(), rhs.pretty()),
            Ty::TyForAll(quantifier, body) => format!("forall {}. {}", quantifier, body.pretty()),
        }
    }

    pub fn is_mono(&self) -> bool {
        match self {
            Ty::TyUnit => true,
            Ty::TyVar(_) | Ty::TyExVar(_) => true,
            Ty::TyArr(lhs, rhs) => lhs.is_mono() && rhs.is_mono(),
            Ty::TyForAll(_, _) => false,
        }
    }

    pub fn occurs_check(&self, var_name: String) -> bool {
        match self {
            Self::TyExVar(name) | Self::TyVar(name) if *name == var_name => true,
            Self::TyArr(lhs, rhs) => {
                lhs.occurs_check(var_name.clone()) || rhs.occurs_check(var_name)
            }
            Self::TyForAll(quantifier, body) => {
                var_name == *quantifier || body.occurs_check(var_name)
            },
            _ => false,
        }
    }

    pub fn make_arr(lhs: Ty, rhs: Ty) -> Ty {
        Ty::TyArr(Box::new(lhs), Box::new(rhs))
    }

    pub fn make_var(var_name: String) -> Ty {
        Self::TyVar(var_name)
    }

    pub fn make_forall(quant_name: String, body: Ty) -> Ty {
        Self::TyForAll(quant_name, Box::new(body))
    }

    pub fn substitute(&self, var_name: &str, with_type: &Ty) -> Ty {
        match self {
            Self::TyVar(sub_var_name) => {
                if sub_var_name == var_name {
                    with_type.clone()
                } else {
                    self.clone()
                }
            }
            Self::TyUnit => self.clone(),
            Self::TyArr(lhs, rhs) => Self::make_arr(
                lhs.substitute(var_name, with_type),
                rhs.substitute(var_name, with_type),
            ),
            Self::TyForAll(quantifier, body) => {
                if quantifier == var_name {
                    Self::make_forall(quantifier.clone(), with_type.clone())
                } else {
                    Self::make_forall(quantifier.clone(), body.substitute(var_name, with_type))
                }
            }
            _ => panic!("subst failed"),
        }
    }
}
