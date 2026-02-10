use chumsky::span::{SimpleSpan, Spanned};

pub type SpannedWrapper<T, S = SimpleSpan> = Spanned<T, S>;


pub enum SurfaceDefinition<'src> {
    FunDef {
        name: Spanned<&'src str>,
        signature: Spanned<SurfaceTy<'src>>,
        body: Spanned<SurfaceExpr<'src>>
    }
}

pub type SurfaceProgram<'src> = Vec<Spanned<SurfaceDefinition<'src>>>;

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceTy<'src> {
    Unit,
    Var(&'src str),
    Arr(Box<Spanned<SurfaceTy<'src>>>, Box<Spanned<SurfaceTy<'src>>>),
    ForAll(Vec<Spanned<&'src str>>, Box<Spanned<SurfaceTy<'src>>>),
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
}
