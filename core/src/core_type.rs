#[derive(Clone, PartialEq, Debug)]
pub enum Ty {
    TyUnit,
    TyVar(String),
    TyExVar(String),
    TyArr(Box<Ty>, Box<Ty>),
    TyForAll(String, Box<Ty>),
    TyCon(String),
    TyApp(Box<Ty>, Box<Ty>),
}

impl Ty {
    pub fn pretty(&self) -> String {
        const PRECEDENCE_LOOSE: u8 = 0; // forall
        const PRECEDENCE_ARR: u8 = 1; // ->
        const PRECEDENCE_APP: u8 = 2; // type application
        const PRECEDENCE_ATOM: u8 = 3; // vars/cons/unit/exvars

        fn print_precedence(t: &Ty) -> u8 {
            match t {
                Ty::TyForAll(_, _) => PRECEDENCE_LOOSE,
                Ty::TyArr(_, _) => PRECEDENCE_ARR,
                Ty::TyApp(_, _) => PRECEDENCE_APP,
                Ty::TyUnit | Ty::TyVar(_) | Ty::TyExVar(_) | Ty::TyCon(_) => PRECEDENCE_ATOM,
            }
        }

        fn traverse_helper(t: &Ty, parent_precedence: u8) -> String {
            let curr_precedence = print_precedence(t);
            let type_fmt = match t {
                Ty::TyUnit => "()".into(),
                Ty::TyVar(name) | Ty::TyExVar(name) | Ty::TyCon(name) => name.clone(),
                Ty::TyArr(lhs, rhs) => {
                    let lhs_fmt = traverse_helper(lhs, PRECEDENCE_APP);
                    let rhs_fmt = traverse_helper(rhs, PRECEDENCE_ARR);
                    format!("{lhs_fmt} -> {rhs_fmt}")
                }
                Ty::TyForAll(_, _) => {
                    let (peeled_quants, ty_body) = t.peel_forall();

                    format!(
                        "∀ {}. {}",
                        peeled_quants.join(" "),
                        traverse_helper(&ty_body, PRECEDENCE_LOOSE)
                    )
                }
                Ty::TyApp(f, x) => {
                    let f_s = traverse_helper(f, PRECEDENCE_APP);
                    let x_s = traverse_helper(x, PRECEDENCE_ATOM);
                    format!("{f_s} {x_s}")
                }
            };

            if curr_precedence < parent_precedence {
                format!("({type_fmt})")
            } else {
                type_fmt
            }
        }

        traverse_helper(self, PRECEDENCE_LOOSE)
    }

    pub fn is_mono(&self) -> bool {
        match self {
            Ty::TyUnit => true,
            Ty::TyVar(_) | Ty::TyExVar(_) | Ty::TyCon(_) => true,
            Ty::TyArr(lhs, rhs) | Ty::TyApp(lhs, rhs) => lhs.is_mono() && rhs.is_mono(),
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
            }
            _ => false,
        }
    }

    pub fn peel_forall(&self) -> (Vec<String>, Self) {
        let mut current_ty = self.clone();
        let mut quants = vec![];
        while let Ty::TyForAll(q, body) = current_ty {
            current_ty = *body;
            quants.push(q.clone());
        }

        (quants, current_ty)
    }

    pub fn quantify_with(&self, qs: &Vec<String>) -> Self {
        let mut current_ty = self.clone();
        for quant in qs.iter().rev() {
            current_ty = Ty::make_forall(quant.clone(), current_ty)
        }

        current_ty
    }

    pub fn make_arr(lhs: Ty, rhs: Ty) -> Ty {
        Ty::TyArr(Box::new(lhs), Box::new(rhs))
    }

    pub fn make_app(lhs: Ty, rhs: Ty) -> Ty {
        Ty::TyApp(Box::new(lhs), Box::new(rhs))
    }

    pub fn make_var(var_name: String) -> Ty {
        Self::TyVar(var_name)
    }

    pub fn make_forall(quant_name: String, body: Ty) -> Ty {
        Self::TyForAll(quant_name, Box::new(body))
    }

    pub fn substitute(&self, var_name: &str, with_type: &Ty) -> Ty {
        match self {
            Self::TyVar(sub_var_name) | Self::TyExVar(sub_var_name) => {
                if sub_var_name == var_name {
                    with_type.clone()
                } else {
                    self.clone()
                }
            }
            Self::TyCon(_) => self.clone(),
            Self::TyUnit => self.clone(),
            Self::TyArr(lhs, rhs) => Self::make_arr(
                lhs.substitute(var_name, with_type),
                rhs.substitute(var_name, with_type),
            ),
            Self::TyApp(lhs, rhs) => Self::make_app(
                lhs.substitute(var_name, with_type),
                rhs.substitute(var_name, with_type),
            ),
            Self::TyForAll(quantifier, body) => {
                if quantifier == var_name {
                    Self::make_forall(quantifier.clone(), (**body).clone())
                } else {
                    Self::make_forall(quantifier.clone(), body.substitute(var_name, with_type))
                }
            }
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Kind {
    KStar,
    KArrow(Box<Kind>, Box<Kind>),
}

impl Kind {
    pub fn make_star() -> Self {
        Kind::KStar
    }

    pub fn make_arrow(lhs: Kind, rhs: Kind) -> Self {
        Kind::KArrow(Box::new(lhs), Box::new(rhs))
    }
}
