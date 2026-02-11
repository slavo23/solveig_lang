use chumsky::{
    input::{Input as _, MappedInput},
    pratt::*,
    prelude::*,
    span::SpanWrap,
};

use crate::ast::{SurfaceDefinition, SurfaceExpr, SurfaceProgram, SurfaceTy};

#[derive(Debug, Clone, PartialEq)]
pub enum Token<'src> {
    Ident(&'src str),
    Parens(Vec<Spanned<Self>>),
    Sig,
    Fun,
    Let,
    In,
    ForAll,
    Dot,
    Colon,
    Semicolon,
    EqBind,
    Arrow,
    FatArrow,
}

pub fn lexer<'src>()
-> impl Parser<'src, &'src str, Vec<Spanned<Token<'src>>>, extra::Err<Rich<'src, char>>> {
    recursive(|token| {
        choice((
            text::ident().map(|s| match s {
                "let" => Token::Let,
                "in" => Token::In,
                "fun" => Token::Fun,
                "forall" => Token::ForAll,
                "sig" => Token::Sig,
                s => Token::Ident(s),
            }),
            just("=>").to(Token::FatArrow),
            just("=").to(Token::EqBind),
            just("->").to(Token::Arrow),
            just(":").to(Token::Colon),
            just(".").to(Token::Dot),
            just(";").to(Token::Semicolon),
            token
                .repeated()
                .collect()
                .delimited_by(just('('), just(')'))
                .labelled("token tree")
                .as_context()
                .map(Token::Parens),
        ))
        .spanned()
        .padded()
    })
    .repeated()
    .collect()
}

fn ty_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceTy<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    recursive(|ty| {
        let ident = select_ref! { Token::Ident(x) => *x };
        let var_ty = ident.map(SurfaceTy::Var);
        let unit_ty = select_ref! {
            Token::Parens(ts) if ts.is_empty() => SurfaceTy::Unit
        };
        let forall_ty = just(Token::ForAll)
            .ignore_then(ident.spanned().repeated().at_least(1).collect::<Vec<_>>())
            .then_ignore(just(Token::Dot))
            .then(ty.clone())
            .map(|(quant, body)| SurfaceTy::ForAll(quant, Box::new(body)));

        let arr_infix = infix(right(0), just(Token::Arrow), |lhs, _, rhs, e| {
            SurfaceTy::Arr(Box::new(lhs), Box::new(rhs)).with_span(e.span())
        });

        choice((
            unit_ty.spanned(),
            var_ty.spanned(),
            forall_ty.spanned(),
            ty.nested_in(select_ref! { Token::Parens(ts) = e => ts.split_spanned(e.span()) }),
        ))
        .pratt((arr_infix,))
    })
}

pub fn expr_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceExpr<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    recursive(|expr| {
        let ident = select_ref! { Token::Ident(x) => *x };
        let unit_expr = select_ref! {
            Token::Parens(ts) if ts.is_empty() => SurfaceExpr::Unit
        };

        let expr_fun = just(Token::Fun)
            .ignore_then(ident.spanned())
            .then_ignore(just(Token::FatArrow))
            .then(expr.clone())
            .map(|(name, body)| SurfaceExpr::Abs(name, Box::new(body)));

        let expr_let = just(Token::Let)
            .ignore_then(ident.spanned())
            .then_ignore(just(Token::EqBind))
            .then(expr.clone())
            .then_ignore(just(Token::In))
            .then(expr.clone())
            .map(|((lhs, rhs), then)| SurfaceExpr::Let(lhs, Box::new(rhs), Box::new(then)));

        let expr_let_anno = just(Token::Let)
            .ignore_then(
                ident
                    .spanned()
                    .then_ignore(just(Token::Colon))
                    .then(ty_parser()),
            )
            .then_ignore(just(Token::EqBind))
            .then(expr.clone())
            .then_ignore(just(Token::In))
            .then(expr.clone())
            .map_with(|(((bind_name, ty_expr), rhs), then), e| {
                let l =
                    SurfaceExpr::Let(bind_name, Box::new(rhs), Box::new(then)).with_span(e.span());

                SurfaceExpr::Ann(Box::new(l), ty_expr)
            })
            .boxed();

        let app_infix = infix(left(0), empty(), |lhs, _, rhs, e| {
            SurfaceExpr::App(Box::new(lhs), Box::new(rhs)).with_span(e.span())
        });

        let variable = ident.map(SurfaceExpr::Var);

        choice((
            expr_let.spanned(),
            expr_let_anno.spanned(),
            expr_fun.spanned(),
            unit_expr.spanned(),
            variable.spanned(),
            expr.nested_in(select_ref! { Token::Parens(ts) = e => ts.split_spanned(e.span()) }),
        ))
        .pratt((app_infix,))
    })
}

pub fn func_definition_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceDefinition<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    let ident = select_ref! { Token::Ident(x) => *x };

    ident
        .spanned()
        .then(ident.spanned().repeated().collect::<Vec<_>>())
        .then_ignore(just(Token::EqBind))
        .then(expr_parser())
        .map(|((name, params), body)| SurfaceDefinition::FunDef { name, body, params })
        .spanned()
}

pub fn signature_definition_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceDefinition<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    let ident = select_ref! { Token::Ident(x) => *x };

    ident
        .spanned()
        .then_ignore(just(Token::Colon))
        .then(ty_parser())
        .map(|(name, ty)| {
            SurfaceDefinition::SigDef { name, ty }
        })
        .spanned()
}


pub fn toplevel_defition_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceDefinition<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    // TODO: add toplevel type choice
    choice((func_definition_parser(), signature_definition_parser()))
}

pub fn program_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    SurfaceProgram<'src>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    toplevel_defition_parser()
        .separated_by(just(Token::Semicolon))
        .collect()
}

#[test]
fn test_lexer() {
    let raw = r"let id: forall a. a -> a = fun b => b in id ()";

    let l = lexer().parse(raw).unwrap();
    let _ = expr_parser()
        .parse(l[..].split_spanned((0..raw.len()).into()))
        .unwrap();
}
