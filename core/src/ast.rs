use std::collections::HashMap;

const PRECEDENCE_LOOSE: u8 = 0;
const PRECEDENCE_ARR: u8 = 1;
const PRECEDENCE_APP: u8 = 2;
const PRECEDENCE_ATOM: u8 = 3;

pub trait Pretty<T> {
    fn pretty(&self) -> String;
    fn get_precedence(t: &T) -> u8;
}

#[derive(Clone, PartialEq, Debug)]
pub enum Kind {
    Var(String),
    Star,
    Arrow(Box<Kind>, Box<Kind>),
}

impl Kind {
    pub fn make_star() -> Self {
        Kind::Star
    }

    pub fn make_var(var: String) -> Self {
        Kind::Var(var)
    }

    pub fn make_arrow(lhs: Kind, rhs: Kind) -> Self {
        Kind::Arrow(Box::new(lhs), Box::new(rhs))
    }
}

impl Pretty<Kind> for Kind {
    fn get_precedence(kind: &Kind) -> u8 {
        match kind {
            Kind::Star | Kind::Var(_) => PRECEDENCE_ATOM,
            Kind::Arrow(_, _) => PRECEDENCE_ARR,
        }
    }

    fn pretty(&self) -> String {
        fn traverse_helper(kind: &Kind, parent_precedence: u8) -> String {
            let curr_precedence = Kind::get_precedence(kind);
            let kind_fmt = match kind {
                Kind::Star => "*".to_string(),
                Kind::Var(k_var) => k_var.to_string(),
                Kind::Arrow(lhs, rhs) => {
                    let lhs_fmt = traverse_helper(lhs, PRECEDENCE_ATOM);
                    let rhs_fmt = traverse_helper(rhs, PRECEDENCE_ARR);
                    format!("{lhs_fmt} -> {rhs_fmt}")
                }
            };

            if curr_precedence < parent_precedence {
                format!("({kind_fmt})")
            } else {
                kind_fmt
            }
        }

        traverse_helper(self, PRECEDENCE_ARR)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum TypeExpr {
    TyUnit,
    TyVar(String),
    TyExVar(String),
    TyArr(Box<TypeExpr>, Box<TypeExpr>),
    TyForAll(String, Kind, Box<TypeExpr>),
    TyCon(String),
    TyApp(Box<TypeExpr>, Box<TypeExpr>),
}

impl TypeExpr {
    pub fn is_mono(&self) -> bool {
        match self {
            TypeExpr::TyForAll(_, _, _) => false,
            TypeExpr::TyArr(lhs, rhs) | TypeExpr::TyApp(lhs, rhs) => lhs.is_mono() && rhs.is_mono(),
            _ => true,
        }
    }

    pub fn occurs_check(&self, var_name: String) -> bool {
        match self {
            Self::TyExVar(name) | Self::TyVar(name) if *name == var_name => true,
            Self::TyArr(lhs, rhs) => {
                lhs.occurs_check(var_name.clone()) || rhs.occurs_check(var_name)
            }
            Self::TyForAll(quantifier, _kind, body) => {
                var_name == *quantifier || body.occurs_check(var_name)
            }
            _ => false,
        }
    }

    pub fn peel_forall(&self) -> (Vec<(String, Kind)>, TypeExpr) {
        let mut current_ty = self.clone();
        let mut quants_with_kinds = vec![];
        while let TypeExpr::TyForAll(q, _kind, body) = current_ty {
            current_ty = *body;
            quants_with_kinds.push((q.clone(), _kind));
        }

        (quants_with_kinds, current_ty)
    }

    pub fn peel_arrow(&self) -> Option<(TypeExpr, TypeExpr)> {
        match self {
            Self::TyArr(fun_ty, arg_ty) => Some((*fun_ty.clone(), *arg_ty.clone())),
            _ => None,
        }
    }

    pub fn make_arr(lhs: TypeExpr, rhs: TypeExpr) -> TypeExpr {
        TypeExpr::TyArr(Box::new(lhs), Box::new(rhs))
    }

    pub fn make_app(lhs: TypeExpr, rhs: TypeExpr) -> TypeExpr {
        TypeExpr::TyApp(Box::new(lhs), Box::new(rhs))
    }

    pub fn make_var(var_name: String) -> TypeExpr {
        Self::TyVar(var_name)
    }

    pub fn make_forall(quant_name: String, kind: Kind, body: TypeExpr) -> TypeExpr {
        TypeExpr::TyForAll(quant_name, kind, Box::new(body))
    }

    pub fn substitute(&self, var_name: &str, with_type: &TypeExpr) -> TypeExpr {
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
            Self::TyForAll(quantifier, kind, body) => {
                if quantifier == var_name {
                    Self::make_forall(quantifier.clone(), kind.clone(), (**body).clone())
                } else {
                    Self::make_forall(
                        quantifier.clone(),
                        kind.clone(),
                        body.substitute(var_name, with_type),
                    )
                }
            }
        }
    }
}

impl Pretty<TypeExpr> for TypeExpr {
    fn get_precedence(t: &TypeExpr) -> u8 {
        match t {
            TypeExpr::TyForAll(_, _, _) => PRECEDENCE_LOOSE,
            TypeExpr::TyArr(_, _) => PRECEDENCE_ARR,
            TypeExpr::TyApp(_, _) => PRECEDENCE_APP,
            TypeExpr::TyUnit | TypeExpr::TyVar(_) | TypeExpr::TyExVar(_) | TypeExpr::TyCon(_) => {
                PRECEDENCE_ATOM
            }
        }
    }

    fn pretty(&self) -> String {
        fn traverse_helper(t: &TypeExpr, parent_precedence: u8) -> String {
            let curr_precedence = TypeExpr::get_precedence(t);
            let type_fmt = match t {
                TypeExpr::TyUnit => "()".into(),
                TypeExpr::TyVar(name) | TypeExpr::TyExVar(name) | TypeExpr::TyCon(name) => {
                    name.clone()
                }
                TypeExpr::TyArr(lhs, rhs) => {
                    let lhs_fmt = traverse_helper(lhs, PRECEDENCE_APP);
                    let rhs_fmt = traverse_helper(rhs, PRECEDENCE_ARR);
                    format!("{lhs_fmt} -> {rhs_fmt}")
                }
                TypeExpr::TyForAll(_, _, _) => {
                    let (kinded_quants, ty_body) = t.peel_forall();
                    let peeled_quants = kinded_quants
                        .iter()
                        .map(|(name, kind)| format!("({name} : {})", kind.pretty()))
                        .collect::<Vec<_>>();

                    format!(
                        "∀ {}. {}",
                        peeled_quants.join(" "),
                        traverse_helper(&ty_body, PRECEDENCE_LOOSE)
                    )
                }
                TypeExpr::TyApp(f, x) => {
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
}

#[derive(Clone, Debug)]
pub struct ExprNode<T, A> {
    pub node: T,
    pub anno: A,
}

impl<T, A> ExprNode<T, A> {
    pub fn new(node: T, anno: A) -> Self {
        Self { node, anno }
    }
}

pub type TermExprNode<A, P = A> = ExprNode<TermExpr<A, P>, A>;
pub type PatternNode<A> = ExprNode<Pattern<A>, A>;
pub type CaseArmNode<A, P = A> = ExprNode<CaseArm<A, P>, P>;

#[derive(Clone, Debug)]
pub enum TermExpr<A, P = A> {
    Unit,
    Var(String),
    App(Box<TermExprNode<A, P>>, Box<TermExprNode<A, P>>),
    Abs(String, Option<TypeExpr>, Box<TermExprNode<A, P>>),
    TyAbs(String, Kind, Box<TermExprNode<A, P>>),
    TyApp(Box<TermExprNode<A, P>>, TypeExpr),
    Anno(Box<TermExprNode<A, P>>, TypeExpr),
    Con(String),
    Case(Box<TermExprNode<A, P>>, Vec<CaseArmNode<A, P>>),
    IfThenElse(
        Box<TermExprNode<A, P>>,
        Box<TermExprNode<A, P>>,
        Box<TermExprNode<A, P>>,
    ),
}

#[derive(Clone, Debug)]
pub enum Pattern<A> {
    Con(String, Vec<PatternNode<A>>),
    Var(String),
    Wild,
}

#[derive(Clone, Debug)]
pub struct CaseArm<A, P = A> {
    pub pattern: PatternNode<P>,
    pub expr: TermExprNode<A, P>,
}

#[derive(Clone, Debug)]
pub struct TermDefinition<A, P = A> {
    pub name: String,
    pub signature: Option<TypeExpr>,
    pub body: TermExprNode<A, P>,
}

#[derive(Debug)]
pub struct ResolvedModule<A, P = A> {
    pub data_defs: Vec<DataDefinition>,
    pub tycon_kinds: HashMap<String, Kind>,
    pub constructor_types: HashMap<String, TypeExpr>,
    pub func_types: HashMap<String, TypeExpr>,
    pub func_defs: Vec<TermDefinition<A, P>>,
}

#[derive(Clone, Debug)]
pub struct DataDefinition {
    pub name: String,
    pub ty_params: Vec<(String, Kind)>,
    pub tycon_kind: Kind,
    pub constructors: Vec<ConstructorDef>,
}

#[derive(Clone, Debug)]
pub struct ConstructorDef {
    pub name: String,
    pub fields: Vec<TypeExpr>,
    pub full_type: TypeExpr,
}
