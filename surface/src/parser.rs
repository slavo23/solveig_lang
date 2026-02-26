use chumsky::{
    input::{Input as _, MappedInput},
    pratt::*,
    prelude::*,
    span::SpanWrap,
};

use crate::ast::{
    SufraceConstructor, SurfaceDefinition, SurfaceExpr, SurfaceMatchArm, SurfacePattern,
    SurfaceProgram, SurfaceTy,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Token<'src> {
    Ident(&'src str),
    UpperIdent(&'src str),
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
    Data,
    Pipe,
    Case,
    Of,
    Uns,
    If,
    Then,
    Else,
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
                "data" => Token::Data,
                "case" => Token::Case,
                "of" => Token::Of,
                "if" => Token::If,
                "then" => Token::Then,
                "else" => Token::Else,
                s => {
                    let is_upper = s.chars().nth(0).unwrap().is_uppercase();
                    if is_upper {
                        Token::UpperIdent(s)
                    } else {
                        Token::Ident(s)
                    }
                }
            }),
            just("=>").to(Token::FatArrow),
            just("=").to(Token::EqBind),
            just("->").to(Token::Arrow),
            just(":").to(Token::Colon),
            just(".").to(Token::Dot),
            just(";").to(Token::Semicolon),
            just("|").to(Token::Pipe),
            just("_").to(Token::Uns),
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

fn ty_atom_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceTy<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    let ident = select_ref! { Token::Ident(x) => *x };
    let upper_ident = select_ref! { Token::UpperIdent(x) => *x };

    let var_ty = ident.map(SurfaceTy::Var).spanned();

    let con_ty = upper_ident.spanned().map(SurfaceTy::Con).spanned();

    let unit_ty = select_ref! {
        Token::Parens(ts) if ts.is_empty() => SurfaceTy::Unit
    }
    .spanned();

    // Parenthesized type is ONE atomic field; inside parens parse the full type
    let parens_ty = ty_parser().nested_in(select_ref! {
        Token::Parens(ts) = e => ts.split_spanned(e.span())
    });

    choice((unit_ty, con_ty, var_ty, parens_ty))
}

fn ty_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceTy<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    recursive(|ty| {
        let ident = select_ref! { Token::Ident(x) => *x };
        let upper_ident = select_ref! { Token::UpperIdent(x) => *x };

        let unit_ty = select_ref! {
            Token::Parens(ts) if ts.is_empty() => SurfaceTy::Unit
        }
        .spanned();

        let var_ty = ident.map(SurfaceTy::Var).spanned();

        let con_ty = upper_ident.spanned().map(SurfaceTy::Con).spanned();

        let parens_ty = ty
            .clone()
            .nested_in(select_ref! { Token::Parens(ts) = e => ts.split_spanned(e.span()) });

        let atom = choice((unit_ty, con_ty, var_ty, parens_ty));

        let app = atom
            .clone()
            .then(atom.clone().repeated().collect::<Vec<_>>())
            .map_with(|(head, rest), e| {
                let mut t = head;
                for arg in rest {
                    t = SurfaceTy::App(Box::new(t), Box::new(arg)).with_span(e.span());
                }
                t
            });

        let arrow = app
            .clone()
            .then(just(Token::Arrow).ignore_then(ty.clone()).or_not())
            .map_with(|(lhs, rhs_opt), e| match rhs_opt {
                None => lhs,
                Some(rhs) => SurfaceTy::Arr(Box::new(lhs), Box::new(rhs)).with_span(e.span()),
            });

        let forall = just(Token::ForAll)
            .ignore_then(ident.spanned().repeated().at_least(1).collect::<Vec<_>>())
            .then_ignore(just(Token::Dot))
            .then(ty.clone())
            .map_with(|(quants, body), e| {
                SurfaceTy::ForAll(quants, Box::new(body)).with_span(e.span())
            });

        choice((forall, arrow))
    })
}

pub fn pattern_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfacePattern<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    recursive(|pattern| {
        let ident = select_ref! { Token::Ident(x) => *x };
        let upper_ident = select_ref! { Token::UpperIdent(x) => *x };
        let wild = just(Token::Uns).to(SurfacePattern::Wild);
        let var = ident.map(SurfacePattern::Var);

        // Atomic patterns for constructor arguments
        let atom_pattern = choice((
            wild.clone().spanned(),
            upper_ident
                .spanned()
                .map(|name| SurfacePattern::Con(name, vec![]))
                .spanned(),
            var.clone().spanned(),
            pattern.clone().nested_in(select_ref! {
                Token::Parens(ts) = e => ts.split_spanned(e.span())
            }),
        ));

        let con_pattern = upper_ident
            .spanned()
            .then(atom_pattern.repeated().collect::<Vec<_>>())
            .map(|(name, args)| SurfacePattern::Con(name, args));

        choice((
            con_pattern.spanned(),
            wild.spanned(),
            var.spanned(),
            pattern.nested_in(select_ref! {
                Token::Parens(ts) = e => ts.split_spanned(e.span())
            }),
        ))
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
        let upper_ident = select_ref! { Token::UpperIdent(x) => *x };
        let unit_expr = select_ref! {
            Token::Parens(ts) if ts.is_empty() => SurfaceExpr::Unit
        };

        let expr_if = just(Token::If)
            .ignore_then(expr.clone())
            .then_ignore(just(Token::Then))
            .then(expr.clone())
            .then_ignore(just(Token::Else))
            .then(expr.clone())
            .map(|((cond, then_branch), else_branch)| {
                SurfaceExpr::IfThenElse(
                    Box::new(cond),
                    Box::new(then_branch),
                    Box::new(else_branch),
                )
            })
            .boxed();

        let ctor_arg = choice((
            unit_expr.clone().spanned(),
            upper_ident
                .spanned()
                .map(|name| SurfaceExpr::Con(name, vec![]))
                .spanned(),
            ident.map(SurfaceExpr::Var).spanned(),
            expr.clone().nested_in(select_ref! {
                Token::Parens(ts) = e => ts.split_spanned(e.span())
            }),
        ));

        let case_arm = pattern_parser()
            .then_ignore(just(Token::FatArrow))
            .then(expr.clone())
            .map(|(pattern, body)| SurfaceMatchArm {
                pattern: pattern,
                expr: body,
            });

        let expr_case = just(Token::Case)
            .ignore_then(expr.clone())
            .then_ignore(just(Token::Of))
            .then(
                case_arm
                    .spanned()
                    .separated_by(just(Token::Pipe))
                    .collect::<Vec<_>>(),
            )
            .map(|(scrutinee, arms)| SurfaceExpr::Case(Box::new(scrutinee), arms))
            .boxed();

        let expr_ctor = upper_ident
            .spanned()
            .then(ctor_arg.repeated().collect::<Vec<_>>())
            .map(|(name, args)| SurfaceExpr::Con(name, args));

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

        let app_infix = infix(left(1), empty(), |lhs, _, rhs, e| {
            SurfaceExpr::App(Box::new(lhs), Box::new(rhs)).with_span(e.span())
        });

        let variable = ident.map(SurfaceExpr::Var);

        choice((
            expr_let.spanned(),
            expr_ctor.spanned(),
            expr_let_anno.spanned(),
            expr_fun.spanned(),
            unit_expr.spanned(),
            variable.spanned(),
            expr_case.spanned(),
            expr_if.spanned(),
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
        .map(|(name, ty)| SurfaceDefinition::SigDef { name, ty })
        .spanned()
}

pub fn data_definition_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceDefinition<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    let ident = select_ref! { Token::Ident(x) => *x };
    let upper_ident = select_ref! { Token::UpperIdent(x) => *x };

    let ctor_variant = upper_ident
        .spanned()
        .then(ty_atom_parser().repeated().collect::<Vec<_>>())
        .map(|(name, ty)| SufraceConstructor { name, params: ty });

    just(Token::Data)
        .ignore_then(upper_ident.spanned())
        .then(ident.spanned().repeated().collect::<Vec<_>>())
        .then_ignore(just(Token::EqBind))
        .then(
            ctor_variant
                .spanned()
                .separated_by(just(Token::Pipe))
                .collect::<Vec<_>>(),
        )
        .map(
            |((ctor_name, params), variants)| SurfaceDefinition::DataDef {
                name: ctor_name,
                ty_params: params,
                constructors: variants,
            },
        )
        .spanned()
}

pub fn toplevel_defition_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceDefinition<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    choice((
        func_definition_parser(),
        signature_definition_parser(),
        data_definition_parser(),
    ))
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
