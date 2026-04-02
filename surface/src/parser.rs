use chumsky::{
    input::{Input as _, MappedInput},
    pratt::*,
    prelude::*,
    span::SpanWrap,
};

use crate::ast::{
    ClassMethod, InstanceMethod, SufraceConstructor, SurfaceDefinition, SurfaceExpr,
    SurfaceMatchArm, SurfacePattern, SurfaceProgram, SurfaceTy,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Token<'src> {
    Ident(&'src str),
    UpperIdent(&'src str),
    Parens(Vec<Spanned<Self>>),
    Brackets(Vec<Spanned<Self>>),
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
    Comma,
    Class,
    Instance,
    Where,
    End,
    ConsSep,
}

pub fn lexer<'src>()
-> impl Parser<'src, &'src str, Vec<Spanned<Token<'src>>>, extra::Err<Rich<'src, char>>> {
    recursive(|token| {
        let token = token.boxed();

        let parens = token
            .clone()
            .repeated()
            .collect()
            .delimited_by(just('('), just(')'))
            .labelled("token tree")
            .as_context()
            .map(Token::Parens);

        let brackets = token
            .clone()
            .repeated()
            .collect()
            .delimited_by(just('['), just(']'))
            .labelled("token tree")
            .as_context()
            .map(Token::Brackets);

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
                "class" => Token::Class,       // NEW
                "instance" => Token::Instance, // NEW
                "where" => Token::Where,       // NEW
                "end" => Token::End,
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
            just("::").to(Token::ConsSep),
            just(":").to(Token::Colon),
            just(".").to(Token::Dot),
            just(";").to(Token::Semicolon),
            just("|").to(Token::Pipe),
            just("_").to(Token::Uns),
            just(",").to(Token::Comma),
            parens,
            brackets,
        ))
        .spanned()
        .padded()
    })
    .repeated()
    .collect()
}

fn merge_spans(left: &SimpleSpan, right: &SimpleSpan) -> SimpleSpan {
    (left.start..right.end).into()
}

fn mk_list_expr<'src>(
    elems: Vec<Spanned<SurfaceExpr<'src>>>,
    list_span: SimpleSpan,
) -> Spanned<SurfaceExpr<'src>> {
    elems.into_iter().rev().fold(
        SurfaceExpr::Con("Nil").with_span(list_span.clone()),
        |acc, elem| {
            let head_span = elem.span.clone();
            let full_span = merge_spans(&head_span, &acc.span);

            let cons_head = SurfaceExpr::App(
                Box::new(SurfaceExpr::Con("Cons").with_span(head_span.clone())),
                Box::new(elem),
            )
            .with_span(head_span);

            SurfaceExpr::App(
                Box::new(cons_head),
                Box::new(acc),
            )
            .with_span(full_span)
        },
    )
}

pub fn class_definition_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceDefinition<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    let ident = select_ref! { Token::Ident(x) => *x };
    let upper_ident = select_ref! { Token::UpperIdent(x) => *x };

    let method_sig = ident
        .spanned()
        .then_ignore(just(Token::Colon))
        .then(ty_parser())
        .map(|(name, ty)| ClassMethod { name, ty });

    just(Token::Class)
        .ignore_then(upper_ident.spanned())
        .then(ident.spanned())
        .then_ignore(just(Token::Where))
        .then(
            method_sig
                .separated_by(just(Token::Semicolon))
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .then_ignore(just(Token::End))
        .map(|((name, ty_param), methods)| SurfaceDefinition::ClassDef {
            name,
            ty_param,
            methods,
        })
        .spanned()
}

pub fn instance_definition_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceDefinition<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    let ident = select_ref! { Token::Ident(x) => *x };
    let upper_ident = select_ref! { Token::UpperIdent(x) => *x };

    let method_def = ident
        .spanned()
        .then(ident.spanned().repeated().collect::<Vec<_>>())
        .then_ignore(just(Token::EqBind))
        .then(expr_parser())
        .map(|((name, params), body)| InstanceMethod { name, params, body });

    let ty_arg = choice((
        upper_ident.spanned().map(SurfaceTy::Con).spanned(),
        ty_parser().nested_in(select_ref! {
            Token::Parens(ts) = e => ts.split_spanned(e.span())
        }),
    ));

    just(Token::Instance)
        .ignore_then(upper_ident.spanned())
        .then(ty_arg)
        .then_ignore(just(Token::Where))
        .then(
            method_def
                .separated_by(just(Token::Semicolon))
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .then_ignore(just(Token::End))
        .map(
            |((class_name, ty_arg), methods)| SurfaceDefinition::InstanceDef {
                class_name,
                ty_arg,
                methods,
            },
        )
        .spanned()
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

    let parens_ty = ty_parser()
        .nested_in(select_ref! {
            Token::Parens(ts) = e => ts.split_spanned(e.span())
        })
        .boxed();

    let list_ty = ty_parser()
        .nested_in(select_ref! {
            Token::Brackets(ts) = e => ts.split_spanned(e.span())
        })
        .map(mk_list_ty)
        .boxed();

    choice((unit_ty, con_ty, var_ty, parens_ty, list_ty)).boxed()
}

fn mk_list_ty<'src>(elem_ty: Spanned<SurfaceTy<'src>>) -> Spanned<SurfaceTy<'src>> {
    let span = elem_ty.span.clone();
    let list_name = "List".with_span(span.clone());

    SurfaceTy::App(
        Box::new(SurfaceTy::Con(list_name).with_span(span.clone())),
        Box::new(elem_ty),
    )
    .with_span(span)
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
            .nested_in(select_ref! {
                Token::Parens(ts) = e => ts.split_spanned(e.span())
            })
            .boxed();

        let list_ty = ty
            .clone()
            .nested_in(select_ref! {
                Token::Brackets(ts) = e => ts.split_spanned(e.span())
            })
            .map(mk_list_ty)
            .boxed();

        let atom = choice((unit_ty, con_ty, var_ty, parens_ty, list_ty)).boxed();

        let app = atom
            .clone()
            .then(atom.repeated().collect::<Vec<_>>())
            .map(|(head, rest)| {
                let mut t = head;
                for arg in rest {
                    let span = t.span.clone();
                    t = SurfaceTy::App(Box::new(t), Box::new(arg)).with_span(span);
                }
                t
            })
            .boxed();

        let arrow = app
            .clone()
            .then(just(Token::Arrow).ignore_then(ty.clone()).or_not())
            .map(|(lhs, rhs_opt)| match rhs_opt {
                None => lhs,
                Some(rhs) => {
                    let span = lhs.span.clone();
                    SurfaceTy::Arr(Box::new(lhs), Box::new(rhs)).with_span(span)
                }
            })
            .boxed();

        let forall = just(Token::ForAll)
            .ignore_then(ident.spanned().repeated().at_least(1).collect::<Vec<_>>())
            .then_ignore(just(Token::Dot))
            .then(ty.clone())
            .map(|(quants, body)| {
                let span = body.span.clone();
                SurfaceTy::ForAll(quants, Box::new(body)).with_span(span)
            })
            .boxed();

        choice((forall, arrow))
    })
}

fn mk_cons_pattern<'src>(
    head: Spanned<SurfacePattern<'src>>,
    tail: Spanned<SurfacePattern<'src>>,
) -> Spanned<SurfacePattern<'src>> {
    let span = head.span.clone();
    SurfacePattern::Con("Cons".with_span(span.clone()), vec![head, tail]).with_span(span)
}

fn mk_list_pattern<'src>(
    elems: Vec<Spanned<SurfacePattern<'src>>>,
    list_span: SimpleSpan,
) -> Spanned<SurfacePattern<'src>> {
    elems.into_iter().rev().fold(
        SurfacePattern::Con("Nil".with_span(list_span.clone()), vec![])
            .with_span(list_span.clone()),
        |acc, elem| {
            let head_span = elem.span.clone();
            let full_span = merge_spans(&head_span, &acc.span);

            SurfacePattern::Con(
                "Cons".with_span(full_span.clone()),
                vec![elem, acc],
            )
            .with_span(full_span)
        },
    )
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

        let wild = just(Token::Uns).to(SurfacePattern::Wild).spanned().boxed();
        let var = ident.map(SurfacePattern::Var).spanned().boxed();

        let list_pattern = pattern
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .nested_in(select_ref! {
                Token::Brackets(ts) = e => ts.split_spanned(e.span())
            })
            .map_with(|elems, e| mk_list_pattern(elems, e.span()))
            .boxed();

        let paren_pattern = pattern
            .clone()
            .nested_in(select_ref! {
                Token::Parens(ts) = e => ts.split_spanned(e.span())
            })
            .boxed();

        let atom_pattern = choice((
            wild.clone(),
            var.clone(),
            list_pattern.clone(),
            upper_ident
                .spanned()
                .map(|name| SurfacePattern::Con(name, vec![]))
                .spanned(),
            paren_pattern,
        ))
        .boxed();

        let con_pattern = upper_ident
            .spanned()
            .then(atom_pattern.clone().repeated().collect::<Vec<_>>())
            .map(|(name, args)| SurfacePattern::Con(name, args))
            .spanned()
            .boxed();

        let pattern_head = choice((con_pattern, list_pattern, wild, var, atom_pattern)).boxed();

        pattern_head
            .clone()
            .then(just(Token::ConsSep).ignore_then(pattern.clone()).or_not())
            .map(|(head, tail_opt)| match tail_opt {
                None => head,
                Some(tail) => mk_cons_pattern(head, tail),
            })
            .boxed()
    })
}

fn mk_cons_expr<'src>(
    head: Spanned<SurfaceExpr<'src>>,
    tail: Spanned<SurfaceExpr<'src>>,
    span: SimpleSpan,
) -> Spanned<SurfaceExpr<'src>> {
    SurfaceExpr::App(
        Box::new(
            SurfaceExpr::App(
                Box::new(SurfaceExpr::Con("Cons").with_span(span.clone())),
                Box::new(head),
            )
            .with_span(span.clone()),
        ),
        Box::new(tail),
    )
    .with_span(span)
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

        let expr_ctor = upper_ident.map(SurfaceExpr::Con);

        let ty = ty_parser().boxed();

        let param_typed = ident
            .spanned()
            .then_ignore(just(Token::Colon))
            .then(ty.clone())
            .nested_in(select_ref! {
                Token::Parens(ts) = e => ts.split_spanned(e.span())
            })
            .map(|(name, ty)| (name, Some(ty)));

        let param_untyped = ident.spanned().map(|name| (name, None));

        let params = choice((param_typed, param_untyped))
            .repeated()
            .at_least(1)
            .collect::<Vec<_>>();

        let expr_fun = just(Token::Fun)
            .ignore_then(params)
            .then_ignore(just(Token::FatArrow))
            .then(expr.clone())
            .map(|(params, body)| SurfaceExpr::Abs(params, Box::new(body)));

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

        let expr_list = expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .nested_in(select_ref! {
                Token::Brackets(ts) = e => ts.split_spanned(e.span())
            })
            .map_with(|elems, e| mk_list_expr(elems, e.span()))
            .boxed();

        let app_infix = infix(left(1), empty(), |lhs, _, rhs, e| {
            SurfaceExpr::App(Box::new(lhs), Box::new(rhs)).with_span(e.span())
        });

        let cons_infix = infix(right(0), just(Token::ConsSep), |lhs, _, rhs, e| {
            mk_cons_expr(lhs, rhs, e.span())
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
            expr_list,
            expr.nested_in(select_ref! { Token::Parens(ts) = e => ts.split_spanned(e.span()) }),
        ))
        .pratt((app_infix, cons_infix))
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
        class_definition_parser(),
        instance_definition_parser(),
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
        .allow_trailing()
        .collect()
}
