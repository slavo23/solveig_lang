use chumsky::span::{SimpleSpan, Spanned};

pub type SpannedWrapper<T, S = SimpleSpan> = Spanned<T, S>;

#[derive(Debug, Clone)]
pub struct ClassMethod<'src> {
    pub name: Spanned<&'src str>,
    pub ty: Spanned<SurfaceTy<'src>>,
}

#[derive(Debug, Clone)]
pub struct InstanceMethod<'src> {
    pub name: Spanned<&'src str>,
    pub params: Vec<Spanned<&'src str>>,
    pub body: Spanned<SurfaceExpr<'src>>,
}

#[derive(Debug)]
pub enum SurfaceDefinition<'src> {
    FunDef {
        name: Spanned<&'src str>,
        body: Spanned<SurfaceExpr<'src>>,
        params: Vec<Spanned<&'src str>>,
    },
    SigDef {
        name: Spanned<&'src str>,
        ty: Spanned<SurfaceTy<'src>>,
    },
    DataDef {
        name: Spanned<&'src str>,
        ty_params: Vec<Spanned<&'src str>>,
        constructors: Vec<Spanned<SufraceConstructor<'src>>>,
    },
    ClassDef {
        name: Spanned<&'src str>,
        ty_param: Spanned<&'src str>,
        methods: Vec<ClassMethod<'src>>,
    },
    InstanceDef {
        class_name: Spanned<&'src str>,
        ty_arg: Spanned<SurfaceTy<'src>>,
        methods: Vec<InstanceMethod<'src>>,
    },
}

#[derive(Debug)]
pub struct SufraceConstructor<'src> {
    pub name: Spanned<&'src str>,
    pub params: Vec<Spanned<SurfaceTy<'src>>>,
}

pub type SurfaceProgram<'src> = Vec<Spanned<SurfaceDefinition<'src>>>;

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceTy<'src> {
    Unit,
    Var(&'src str),
    Arr(Box<Spanned<SurfaceTy<'src>>>, Box<Spanned<SurfaceTy<'src>>>),
    ForAll(Vec<Spanned<&'src str>>, Box<Spanned<SurfaceTy<'src>>>),
    Con(Spanned<&'src str>),
    App(Box<Spanned<SurfaceTy<'src>>>, Box<Spanned<SurfaceTy<'src>>>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SurfacePattern<'src> {
    Var(&'src str),
    Con(Spanned<&'src str>, Vec<Spanned<SurfacePattern<'src>>>),
    Wild,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceMatchArm<'src> {
    pub pattern: Spanned<SurfacePattern<'src>>,
    pub expr: Spanned<SurfaceExpr<'src>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceBinOp {
    Plus,
    Minus,
    Mult,
    Div,

    Less,
    Greater,
    LessOrEqual,
    GreaterOrEqual,
    Equal,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceUnOp {
    Neg,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceLiteral {
    Int(i64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceExpr<'src> {
    Unit,
    Literal(SurfaceLiteral),
    Var(&'src str),
    App(
        Box<Spanned<SurfaceExpr<'src>>>,
        Box<Spanned<SurfaceExpr<'src>>>,
    ),
    Abs(
        Vec<(Spanned<&'src str>, Option<Spanned<SurfaceTy<'src>>>)>,
        Box<Spanned<SurfaceExpr<'src>>>,
    ),
    Ann(Box<Spanned<SurfaceExpr<'src>>>, Spanned<SurfaceTy<'src>>),
    Let(
        Spanned<&'src str>,
        Box<Spanned<SurfaceExpr<'src>>>,
        Box<Spanned<SurfaceExpr<'src>>>,
    ),
    Con(&'src str),
    Case(
        Box<Spanned<SurfaceExpr<'src>>>,
        Vec<Spanned<SurfaceMatchArm<'src>>>,
    ),
    IfThenElse(
        Box<Spanned<SurfaceExpr<'src>>>,
        Box<Spanned<SurfaceExpr<'src>>>,
        Box<Spanned<SurfaceExpr<'src>>>,
    ),
    BinaryOperator(
        SurfaceBinOp,
        Box<Spanned<SurfaceExpr<'src>>>,
        Box<Spanned<SurfaceExpr<'src>>>,
    ),
    UnaryOperator(SurfaceUnOp, Box<Spanned<SurfaceExpr<'src>>>),
}
