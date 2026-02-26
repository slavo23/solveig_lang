use chumsky::span::{SimpleSpan, Spanned};

pub type SpannedWrapper<T, S = SimpleSpan> = Spanned<T, S>;

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
pub enum SurfaceExpr<'src> {
    Unit,
    Var(&'src str),
    App(
        Box<Spanned<SurfaceExpr<'src>>>,
        Box<Spanned<SurfaceExpr<'src>>>,
    ),
    Abs(Spanned<&'src str>, Box<Spanned<SurfaceExpr<'src>>>),
    Ann(Box<Spanned<SurfaceExpr<'src>>>, Spanned<SurfaceTy<'src>>),
    Let(
        Spanned<&'src str>,
        Box<Spanned<SurfaceExpr<'src>>>,
        Box<Spanned<SurfaceExpr<'src>>>,
    ),
    Con(Spanned<&'src str>, Vec<Spanned<SurfaceExpr<'src>>>),
    Case(
        Box<Spanned<SurfaceExpr<'src>>>,
        Vec<Spanned<SurfaceMatchArm<'src>>>,
    ),
    IfThenElse(
        Box<Spanned<SurfaceExpr<'src>>>,
        Box<Spanned<SurfaceExpr<'src>>>,
        Box<Spanned<SurfaceExpr<'src>>>,
    ),
}
