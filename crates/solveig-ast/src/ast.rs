use std::collections::HashMap;

use solveig_base::Symbol;

const PRECEDENCE_LOOSE: u8 = 0;
const PRECEDENCE_ARR: u8 = 1;
const PRECEDENCE_APP: u8 = 2;
const PRECEDENCE_ATOM: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId {
    pub module: ModuleId,
    pub name: Symbol,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TyVarId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EvarId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TyConId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CtorId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KindMetaId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum VarRef {
    Local(Symbol),
    Global(DefId),
}

pub trait Pretty<T> {
    fn pretty(&self) -> String;
    fn get_precedence(t: &T) -> u8;
}

#[derive(Clone, PartialEq, Debug, Eq, Hash)]
pub enum Kind {
    Meta(KindMetaId),
    Star,
    Arrow(Box<Kind>, Box<Kind>),
}

impl Kind {
    pub fn make_star() -> Self {
        Kind::Star
    }

    pub fn make_meta(id: KindMetaId) -> Self {
        Kind::Meta(id)
    }

    pub fn make_arrow(lhs: Kind, rhs: Kind) -> Self {
        Kind::Arrow(Box::new(lhs), Box::new(rhs))
    }
}

impl Pretty<Kind> for Kind {
    fn get_precedence(kind: &Kind) -> u8 {
        match kind {
            Kind::Star | Kind::Meta(_) => PRECEDENCE_ATOM,
            Kind::Arrow(_, _) => PRECEDENCE_ARR,
        }
    }

    fn pretty(&self) -> String {
        fn traverse_helper(kind: &Kind, parent_precedence: u8) -> String {
            let curr_precedence = Kind::get_precedence(kind);
            let kind_fmt = match kind {
                Kind::Star => "*".to_string(),
                Kind::Meta(id) => format!("?k{}", id.0),
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

#[derive(Clone, PartialEq, Debug, Eq, Hash)]
pub enum Ty {
    TyUnit,
    TyVar(String),
    TyExVar(EvarId),
    TyArr(Box<Ty>, Box<Ty>),
    TyForAll(String, Kind, Box<Ty>),
    TyCon(String),
    TyApp(Box<Ty>, Box<Ty>),
    Qualified(Vec<Predicate>, Box<Ty>),
}

pub type TypeExpr = Ty;

pub fn legacy_evar_name(id: EvarId) -> String {
    format!("a^{}", id.0)
}

pub fn legacy_tyvar_name(base: &str, id: TyVarId) -> String {
    format!("{}{}", base, id.0)
}

impl Ty {
    pub fn is_mono(&self) -> bool {
        match self {
            TypeExpr::TyForAll(_, _, _) => false,
            TypeExpr::TyArr(lhs, rhs) | TypeExpr::TyApp(lhs, rhs) => lhs.is_mono() && rhs.is_mono(),
            _ => true,
        }
    }

    pub fn occurs_check(&self, evar_id: EvarId) -> bool {
        match self {
            Self::TyExVar(id) if *id == evar_id => true,

            Self::TyArr(lhs, rhs) | Self::TyApp(lhs, rhs) => {
                lhs.occurs_check(evar_id) || rhs.occurs_check(evar_id)
            }

            Self::TyForAll(_, _kind, body) => body.occurs_check(evar_id),

            Self::Qualified(preds, body) => {
                preds
                    .iter()
                    .any(|pred| pred.args.iter().any(|arg| arg.occurs_check(evar_id)))
                    || body.occurs_check(evar_id)
            }

            _ => false,
        }
    }

    pub fn peel_qualified(&self) -> (Vec<Predicate>, TypeExpr) {
        match self {
            TypeExpr::Qualified(preds, body) => (preds.clone(), (**body).clone()),
            other => (Vec::new(), other.clone()),
        }
    }

    pub fn peel_scheme_like(&self) -> (Vec<(String, Kind)>, Vec<Predicate>, TypeExpr) {
        let (quants, rest) = self.peel_forall();
        let (preds, ty) = rest.peel_qualified();
        (quants, preds, ty)
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

    pub fn make_var_id(base: &str, id: TyVarId) -> TypeExpr {
        Self::TyVar(legacy_tyvar_name(base, id))
    }

    pub fn make_evar(id: EvarId) -> TypeExpr {
        Self::TyExVar(id)
    }

    pub fn make_forall(quant_name: String, kind: Kind, body: TypeExpr) -> TypeExpr {
        TypeExpr::TyForAll(quant_name, kind, Box::new(body))
    }

    pub fn make_qualified(predicates: Vec<Predicate>, body: TypeExpr) -> TypeExpr {
        TypeExpr::Qualified(predicates, Box::new(body))
    }

    pub fn substitute(&self, var_name: &str, with_type: &TypeExpr) -> TypeExpr {
        match self {
            Self::TyVar(sub_var_name) => {
                if sub_var_name == var_name {
                    with_type.clone()
                } else {
                    self.clone()
                }
            }

            Self::TyCon(_) | Self::TyExVar(_) | Self::TyUnit => self.clone(),

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

            Self::Qualified(preds, body) => {
                let new_preds = preds
                    .iter()
                    .map(|pred| Predicate {
                        class_name: pred.class_name.clone(),
                        args: pred
                            .args
                            .iter()
                            .map(|arg| arg.substitute(var_name, with_type))
                            .collect(),
                    })
                    .collect();

                Self::make_qualified(new_preds, body.substitute(var_name, with_type))
            }
        }
    }

    pub fn substitute_evar(&self, evar_id: EvarId, with_type: &TypeExpr) -> TypeExpr {
        match self {
            Self::TyExVar(id) => {
                if *id == evar_id {
                    with_type.clone()
                } else {
                    self.clone()
                }
            }

            Self::TyVar(_) | Self::TyCon(_) | Self::TyUnit => self.clone(),

            Self::TyArr(lhs, rhs) => Self::make_arr(
                lhs.substitute_evar(evar_id, with_type),
                rhs.substitute_evar(evar_id, with_type),
            ),

            Self::TyApp(lhs, rhs) => Self::make_app(
                lhs.substitute_evar(evar_id, with_type),
                rhs.substitute_evar(evar_id, with_type),
            ),

            Self::TyForAll(quantifier, kind, body) => Self::make_forall(
                quantifier.clone(),
                kind.clone(),
                body.substitute_evar(evar_id, with_type),
            ),

            Self::Qualified(preds, body) => {
                let new_preds = preds
                    .iter()
                    .map(|pred| Predicate {
                        class_name: pred.class_name.clone(),
                        args: pred
                            .args
                            .iter()
                            .map(|arg| arg.substitute_evar(evar_id, with_type))
                            .collect(),
                    })
                    .collect();

                Self::make_qualified(new_preds, body.substitute_evar(evar_id, with_type))
            }
        }
    }
}

impl Pretty<Ty> for Ty {
    fn get_precedence(t: &Ty) -> u8 {
        match t {
            TypeExpr::TyForAll(_, _, _) | TypeExpr::Qualified(_, _) => PRECEDENCE_LOOSE,
            TypeExpr::TyArr(_, _) => PRECEDENCE_ARR,
            TypeExpr::TyApp(_, _) => PRECEDENCE_APP,
            TypeExpr::TyUnit | TypeExpr::TyVar(_) | TypeExpr::TyExVar(_) | TypeExpr::TyCon(_) => {
                PRECEDENCE_ATOM
            }
        }
    }

    fn pretty(&self) -> String {
        fn traverse_helper(t: &Ty, parent_precedence: u8) -> String {
            let curr_precedence = TypeExpr::get_precedence(t);

            let type_fmt = match t {
                TypeExpr::TyUnit => "()".into(),
                TypeExpr::TyVar(name) => name.clone(),
                TypeExpr::TyExVar(id) => legacy_evar_name(*id),
                TypeExpr::TyCon(name) => name.split('.').last().unwrap().to_string(),

                TypeExpr::TyArr(lhs, rhs) => {
                    let lhs_fmt = traverse_helper(lhs, PRECEDENCE_APP);
                    let rhs_fmt = traverse_helper(rhs, PRECEDENCE_ARR);
                    format!("{lhs_fmt} -> {rhs_fmt}")
                }

                TypeExpr::TyForAll(_, _, _) => {
                    let (kinded_quants, rest) = t.peel_forall();

                    let peeled_quants = kinded_quants
                        .iter()
                        .map(|(name, kind)| format!("({name} : {})", kind.pretty()))
                        .collect::<Vec<_>>();

                    format!(
                        "∀ {}. {}",
                        peeled_quants.join(" "),
                        traverse_helper(&rest, PRECEDENCE_LOOSE)
                    )
                }

                TypeExpr::Qualified(preds, body) => {
                    let preds_fmt = if preds.len() == 1 {
                        preds[0].pretty()
                    } else {
                        format!(
                            "({})",
                            preds
                                .iter()
                                .map(|p| p.pretty())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };

                    format!("{preds_fmt} => {}", traverse_helper(body, PRECEDENCE_LOOSE))
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

impl Predicate {
    pub fn pretty(&self) -> String {
        let class_name = self
            .class_name
            .split('.')
            .last()
            .unwrap_or(&self.class_name);

        let args = self
            .args
            .iter()
            .map(|arg| {
                let s = arg.pretty();
                match arg {
                    TypeExpr::TyArr(_, _)
                    | TypeExpr::TyForAll(_, _, _)
                    | TypeExpr::Qualified(_, _) => format!("({s})"),
                    _ => s,
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        if args.is_empty() {
            class_name.to_string()
        } else {
            format!("{class_name} {args}")
        }
    }
}

impl Scheme {
    pub fn pretty(&self) -> String {
        let mut ty = self.ty.clone();

        if !self.predicates.is_empty() {
            ty = TypeExpr::make_qualified(self.predicates.clone(), ty);
        }

        for (name, kind) in self.quantifiers.iter().rev() {
            ty = TypeExpr::make_forall(name.clone(), kind.clone(), ty);
        }

        ty.pretty()
    }

    pub fn mono(ty: TypeExpr) -> Self {
        Self {
            quantifiers: Vec::new(),
            predicates: Vec::new(),
            ty,
        }
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
pub enum Literal {
    Int(i64),
}

#[derive(Clone, Debug, Copy)]
pub enum BinOp {
    Plus,
    Minus,
    Mult,
    Div,
    Mod,

    Less,
    Greater,
    LessOrEqual,
    GreaterOrEqual,
    Equal,
}

#[derive(Clone, Debug, Copy)]
pub enum UnOp {
    Neg,
}

#[derive(Clone, Debug)]
pub enum TermExpr<A, P = A> {
    Unit,
    Var(String),
    Lit(Literal),
    Hole,
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
    BinaryOperator(BinOp, Box<TermExprNode<A, P>>, Box<TermExprNode<A, P>>),
    UnaryOperator(UnOp, Box<TermExprNode<A, P>>),
    Let(String, Box<TermExprNode<A, P>>, Box<TermExprNode<A, P>>),
}

#[derive(Clone, Debug)]
pub enum Pattern<A> {
    Con(String, Vec<PatternNode<A>>),
    Lit(Literal),
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
    pub signature: Option<Scheme>,
    pub body: TermExprNode<A, P>,
}

#[derive(Clone, Debug)]
pub struct ResolvedModule<A, P = A> {
    pub data_defs: Vec<DataDefinition>,
    pub class_defs: Vec<ClassDefinition>,
    pub instance_defs: Vec<InstanceDefinition<A, P>>,
    pub tycon_kinds: HashMap<String, Kind>,
    pub constructor_types: HashMap<String, TypeExpr>,
    pub func_schemes: HashMap<String, Scheme>,
    pub func_defs: Vec<TermDefinition<A, P>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataDefinition {
    pub name: String,
    pub ty_params: Vec<(String, Kind)>,
    pub tycon_kind: Kind,
    pub constructors: Vec<ConstructorDef>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TyConDefinition {
    Data(DataDefinition),
    Primitive,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TyConInfo {
    pub name: Symbol,
    pub kind: Kind,
    pub definition: TyConDefinition,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TyConTable {
    entries: Vec<TyConInfo>,
}

impl TyConTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, info: TyConInfo) -> TyConId {
        let id = TyConId(self.entries.len() as u32);
        self.entries.push(info);
        id
    }

    pub fn get(&self, id: TyConId) -> Option<&TyConInfo> {
        self.entries.get(id.0 as usize)
    }

    pub fn iter(&self) -> impl Iterator<Item = (TyConId, &TyConInfo)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, info)| (TyConId(index as u32), info))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConstructorDef {
    pub name: String,
    pub fields: Vec<TypeExpr>,
    pub full_type: TypeExpr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CtorInfo {
    pub name: Symbol,
    pub tycon: TyConId,
    pub fields: Vec<TypeExpr>,
    pub full_type: TypeExpr,
    pub tag: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CtorTable {
    entries: Vec<CtorInfo>,
}

impl CtorTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, info: CtorInfo) -> CtorId {
        let id = CtorId(self.entries.len() as u32);
        self.entries.push(info);
        id
    }

    pub fn get(&self, id: CtorId) -> Option<&CtorInfo> {
        self.entries.get(id.0 as usize)
    }

    pub fn iter(&self) -> impl Iterator<Item = (CtorId, &CtorInfo)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, info)| (CtorId(index as u32), info))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Predicate {
    pub class_name: String,
    pub args: Vec<TypeExpr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scheme {
    pub quantifiers: Vec<(String, Kind)>,
    pub predicates: Vec<Predicate>,
    pub ty: TypeExpr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassMethodDef {
    pub name: String,
    pub scheme: Scheme,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassDefinition {
    pub name: String,
    pub params: Vec<(String, Kind)>,
    pub superclasses: Vec<Predicate>,
    pub methods: Vec<ClassMethodDef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassInfo {
    pub name: Symbol,
    pub definition: ClassDefinition,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassTable {
    entries: Vec<ClassInfo>,
}

impl ClassTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, info: ClassInfo) -> ClassId {
        let id = ClassId(self.entries.len() as u32);
        self.entries.push(info);
        id
    }

    pub fn get(&self, id: ClassId) -> Option<&ClassInfo> {
        self.entries.get(id.0 as usize)
    }

    pub fn iter(&self) -> impl Iterator<Item = (ClassId, &ClassInfo)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, info)| (ClassId(index as u32), info))
    }
}

#[derive(Clone, Debug)]
pub struct InstanceMethodDef<A, P = A> {
    pub name: String,
    pub body: TermExprNode<A, P>,
}

#[derive(Clone, Debug)]
pub struct InstanceDefinition<A, P = A> {
    pub context: Vec<Predicate>,
    pub head: Predicate,
    pub methods: Vec<InstanceMethodDef<A, P>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use solveig_base::Session;

    #[test]
    fn identity_tables_allocate_dense_stable_ids() {
        let session = Session::new();

        let mut tycons = TyConTable::new();
        let bool_id = tycons.insert(TyConInfo {
            name: session.intern("Bool"),
            kind: Kind::Star,
            definition: TyConDefinition::Primitive,
        });
        let list_id = tycons.insert(TyConInfo {
            name: session.intern("List"),
            kind: Kind::make_arrow(Kind::Star, Kind::Star),
            definition: TyConDefinition::Primitive,
        });

        assert_eq!(bool_id, TyConId(0));
        assert_eq!(list_id, TyConId(1));
        assert_eq!(
            session.str(
                tycons
                    .get(bool_id)
                    .expect("ICE: inserted Bool type constructor must be present")
                    .name
            ),
            "Bool"
        );

        let mut ctors = CtorTable::new();
        let false_id = ctors.insert(CtorInfo {
            name: session.intern("False"),
            tycon: bool_id,
            fields: Vec::new(),
            full_type: TypeExpr::TyCon("Bool".to_string()),
            tag: 0,
        });
        let true_id = ctors.insert(CtorInfo {
            name: session.intern("True"),
            tycon: bool_id,
            fields: Vec::new(),
            full_type: TypeExpr::TyCon("Bool".to_string()),
            tag: 1,
        });

        assert_eq!(false_id, CtorId(0));
        assert_eq!(true_id, CtorId(1));
        assert_eq!(
            ctors
                .get(true_id)
                .expect("ICE: inserted True constructor must be present")
                .tag,
            1
        );

        let mut classes = ClassTable::new();
        let eq_id = classes.insert(ClassInfo {
            name: session.intern("Eq"),
            definition: ClassDefinition {
                name: "Eq".to_string(),
                params: vec![("a".to_string(), Kind::Star)],
                superclasses: Vec::new(),
                methods: Vec::new(),
            },
        });

        assert_eq!(eq_id, ClassId(0));
        assert_eq!(
            classes.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            vec![eq_id]
        );
    }

    #[test]
    fn legacy_evar_bridge_round_trips_ids() {
        let id = EvarId(42);

        assert_eq!(legacy_evar_name(id), "a^42");
        assert_eq!(TypeExpr::make_evar(id), TypeExpr::TyExVar(id));
    }

    #[test]
    fn occurs_check_only_matches_existential_ids() {
        let needle = EvarId(2);

        assert!(TypeExpr::make_evar(needle).occurs_check(needle));
        assert!(!TypeExpr::make_evar(EvarId(3)).occurs_check(needle));
        assert!(!TypeExpr::TyVar(legacy_evar_name(needle)).occurs_check(needle));
    }

    #[test]
    fn legacy_tyvar_bridge_names_generated_ids() {
        let id = TyVarId(9);

        assert_eq!(legacy_tyvar_name("t", id), "t9");
        assert_eq!(
            TypeExpr::make_var_id("t", id),
            TypeExpr::TyVar("t9".to_string())
        );
    }
}
