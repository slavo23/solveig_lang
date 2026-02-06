use chumsky::{
    input::{Input as _, MappedInput},
    pratt::*,
    prelude::*,
};

#[derive(Debug, Clone, PartialEq)]
enum Token<'src> {
    Ident(&'src str),
    Parens(Vec<Spanned<Self>>),
    Sig,
    Fun,
    Let,
    In,
    ForAll,
    Dot,
    Colon,
    EqBind,
    Arrow,
    FatArrow,
    NewLine,
}

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
    Abs(String, Box<Spanned<SurfaceExpr<'src>>>),
    Ann(Box<Spanned<SurfaceExpr<'src>>>, SurfaceTy<'src>),
    Let(
        &'src str,
        Box<Spanned<SurfaceExpr<'src>>>,
        Box<Spanned<SurfaceExpr<'src>>>,
    ),
}

fn lexer<'src>()
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

#[test]
fn test_lexer() {
    let raw = r"forall a b. a -> (b -> a) -> ()";

    let l = lexer().parse(raw).unwrap();
    let ty_expr = ty_parser()
        .parse(l[..].split_spanned((0..raw.len()).into()))
        .unwrap();

    println!("{:?}", ty_expr);

    assert!(false);
}
