use chumsky::{
    input::{Input as _, MappedInput},
    pratt::*,
    prelude::*,
    span::SpanWrap,
};

use crate::ast::{
    ClassMethod, ImportDecl, ImportSpec, InstanceMethod, ModuleName, NameRef, ParsedModule,
    SufraceConstructor, SurfaceBinOp, SurfaceDefinition, SurfaceExpr, SurfaceLiteral,
    SurfaceMatchArm, SurfacePattern, SurfaceProgram, SurfaceTy, SurfaceUnOp,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Token<'src> {
    IntLit(i64),
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
    Minus,
    Plus,
    Multiply,
    Divide,
    Less,
    Greater,
    GreaterOrEqual,
    LessOrEqual,
    Equal,
    Import,
    As,
}

pub fn lexer<'src>()
-> impl Parser<'src, &'src str, Vec<Spanned<Token<'src>>>, extra::Err<Rich<'src, char>>> {
    recursive(|token| {
        let token = token.boxed();

        let line_comment = just("--")
            .ignore_then(none_of("\n").repeated())
            .ignored()
            .boxed();

        let block_comment = recursive(|block_comment| {
            just("{-")
                .ignore_then(
                    choice((
                        block_comment,
                        just("{-").or(just("-}")).not().ignore_then(any()).ignored(),
                    ))
                    .repeated(),
                )
                .then_ignore(just("-}"))
                .ignored()
        })
        .boxed();

        let padding = choice((
            text::whitespace().at_least(1).ignored(),
            line_comment,
            block_comment,
        ))
        .repeated()
        .ignored()
        .boxed();

        let int_lit = text::int(10)
            .from_str::<i64>()
            .unwrapped()
            .map(Token::IntLit);

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
            int_lit,
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
                "class" => Token::Class,
                "instance" => Token::Instance,
                "where" => Token::Where,
                "#end" => Token::End, // remove later
                "import" => Token::Import,
                "as" => Token::As,
                s => {
                    let is_upper = s.chars().nth(0).unwrap().is_uppercase();
                    if is_upper {
                        Token::UpperIdent(s)
                    } else {
                        Token::Ident(s)
                    }
                }
            }),
            just(">=").to(Token::GreaterOrEqual),
            just(">").to(Token::Greater),
            just("<=").to(Token::LessOrEqual),
            just("<").to(Token::Less),
            just("=>").to(Token::FatArrow),
            just("==").to(Token::Equal),
            just("=").to(Token::EqBind),
            just("->").to(Token::Arrow),
            just("::").to(Token::ConsSep),
            just(":").to(Token::Colon),
            just(".").to(Token::Dot),
            just(";").to(Token::Semicolon),
            just("|").to(Token::Pipe),
            just("_").to(Token::Uns),
            just(",").to(Token::Comma),
            just("+").to(Token::Plus),
            just("-").to(Token::Minus),
            just("*").to(Token::Multiply),
            just("/").to(Token::Divide),
            parens,
            brackets,
        ))
        .spanned()
        .padded_by(padding.clone())
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
        SurfaceExpr::Con(qref_spanned(&["prelude", "list", "Nil"], list_span.clone()))
            .with_span(list_span.clone()),
        |acc, elem| {
            let head_span = elem.span.clone();
            let full_span = merge_spans(&head_span, &acc.span);

            let cons_head = SurfaceExpr::App(
                Box::new(
                    SurfaceExpr::Con(qref_spanned(
                        &["prelude", "list", "Cons"],
                        head_span.clone(),
                    ))
                    .with_span(head_span.clone()),
                ),
                Box::new(elem),
            )
            .with_span(head_span);

            SurfaceExpr::App(Box::new(cons_head), Box::new(acc)).with_span(full_span)
        },
    )
}

fn mk_list_ty<'src>(elem_ty: Spanned<SurfaceTy<'src>>) -> Spanned<SurfaceTy<'src>> {
    let span = elem_ty.span.clone();

    SurfaceTy::App(
        Box::new(
            SurfaceTy::Con(qref_spanned(&["prelude", "list", "List"], span.clone()))
                .with_span(span.clone()),
        ),
        Box::new(elem_ty),
    )
    .with_span(span)
}

fn mk_cons_pattern<'src>(
    head: Spanned<SurfacePattern<'src>>,
    tail: Spanned<SurfacePattern<'src>>,
) -> Spanned<SurfacePattern<'src>> {
    let span = head.span.clone();
    SurfacePattern::Con(
        qref_spanned(&["prelude", "list", "Cons"], span.clone()),
        vec![head, tail],
    )
    .with_span(span)
}

fn mk_list_pattern<'src>(
    elems: Vec<Spanned<SurfacePattern<'src>>>,
    list_span: SimpleSpan,
) -> Spanned<SurfacePattern<'src>> {
    elems.into_iter().rev().fold(
        SurfacePattern::Con(
            qref_spanned(&["prelude", "list", "Nil"], list_span.clone()),
            vec![],
        )
        .with_span(list_span.clone()),
        |acc, elem| {
            let head_span = elem.span.clone();
            let full_span = merge_spans(&head_span, &acc.span);

            SurfacePattern::Con(
                qref_spanned(&["prelude", "list", "Cons"], full_span.clone()),
                vec![elem, acc],
            )
            .with_span(full_span)
        },
    )
}

fn mk_cons_expr<'src>(
    head: Spanned<SurfaceExpr<'src>>,
    tail: Spanned<SurfaceExpr<'src>>,
    span: SimpleSpan,
) -> Spanned<SurfaceExpr<'src>> {
    SurfaceExpr::App(
        Box::new(
            SurfaceExpr::App(
                Box::new(
                    SurfaceExpr::Con(qref_spanned(&["prelude", "list", "Cons"], span.clone()))
                        .with_span(span.clone()),
                ),
                Box::new(head),
            )
            .with_span(span.clone()),
        ),
        Box::new(tail),
    )
    .with_span(span)
}

// pub fn class_definition_parser<'tokens, 'src: 'tokens>() -> impl Parser<
//     'tokens,
//     MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
//     Spanned<SurfaceDefinition<'src>>,
//     extra::Err<Rich<'tokens, Token<'src>>>,
// > {
//     let ident = select_ref! { Token::Ident(x) => *x };
//     let upper_ident = select_ref! { Token::UpperIdent(x) => *x };

//     let method_sig = ident
//         .spanned()
//         .then_ignore(just(Token::Colon))
//         .then(ty_parser())
//         .map(|(name, ty)| ClassMethod { name, ty });

//     just(Token::Class)
//         .ignore_then(upper_ident.spanned())
//         .then(ident.spanned())
//         .then_ignore(just(Token::Where))
//         .then(
//             method_sig
//                 .separated_by(just(Token::Semicolon))
//                 .allow_trailing()
//                 .collect::<Vec<_>>(),
//         )
//         .then_ignore(just(Token::End))
//         .map(|((name, ty_param), methods)| SurfaceDefinition::ClassDef {
//             name,
//             ty_param,
//             methods,
//         })
//         .spanned()
// }

// pub fn instance_definition_parser<'tokens, 'src: 'tokens>() -> impl Parser<
//     'tokens,
//     MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
//     Spanned<SurfaceDefinition<'src>>,
//     extra::Err<Rich<'tokens, Token<'src>>>,
// > {
//     let ident = select_ref! { Token::Ident(x) => *x };
//     let upper_ident = select_ref! { Token::UpperIdent(x) => *x };

//     let method_def = ident
//         .spanned()
//         .then(ident.spanned().repeated().collect::<Vec<_>>())
//         .then_ignore(just(Token::EqBind))
//         .then(expr_parser())
//         .map(|((name, params), body)| InstanceMethod { name, params, body });

//     let ty_arg = choice((
//         upper_ident.spanned().map(SurfaceTy::Con).spanned(),
//         ty_parser().nested_in(select_ref! {
//             Token::Parens(ts) = e => ts.split_spanned(e.span())
//         }),
//     ));

//     just(Token::Instance)
//         .ignore_then(upper_ident.spanned())
//         .then(ty_arg)
//         .then_ignore(just(Token::Where))
//         .then(
//             method_def
//                 .separated_by(just(Token::Semicolon))
//                 .allow_trailing()
//                 .collect::<Vec<_>>(),
//         )
//         .then_ignore(just(Token::End))
//         .map(
//             |((class_name, ty_arg), methods)| SurfaceDefinition::InstanceDef {
//                 class_name,
//                 ty_arg,
//                 methods,
//             },
//         )
//         .spanned()
// }

fn ty_atom_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceTy<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    let ident = select_ref! { Token::Ident(x) => *x };
    // let upper_ident = select_ref! { Token::UpperIdent(x) => *x };

    let var_ty = ident.map(SurfaceTy::Var).spanned();
    let con_ty = ctor_ref_parser().spanned().map(SurfaceTy::Con).spanned();

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

fn ty_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceTy<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    recursive(|ty| {
        let ident = select_ref! { Token::Ident(x) => *x };

        let unit_ty = select_ref! {
            Token::Parens(ts) if ts.is_empty() => SurfaceTy::Unit
        }
        .spanned();

        let var_ty = ident.map(SurfaceTy::Var).spanned();
        let con_ty = ctor_ref_parser().spanned().map(SurfaceTy::Con).spanned();

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

pub fn pattern_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfacePattern<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    recursive(|pattern| {
        let ident = select_ref! { Token::Ident(x) => *x };

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

        let nullary_ctor = ctor_ref_parser()
            .spanned()
            .map(|name| SurfacePattern::Con(name, vec![]))
            .spanned();

        let atom_pattern = choice((
            wild.clone(),
            var.clone(),
            list_pattern.clone(),
            nullary_ctor,
            paren_pattern,
        ))
        .boxed();

        let con_pattern = ctor_ref_parser()
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

pub fn expr_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Spanned<SurfaceExpr<'src>>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    recursive(|expr| {
        let ident = select_ref! { Token::Ident(x) => *x };
        // let upper_ident = select_ref! { Token::UpperIdent(x) => *x };
        let unit_expr = select_ref! {
            Token::Parens(ts) if ts.is_empty() => SurfaceExpr::Unit
        };
        let expr_int =
            select_ref! { Token::IntLit(n) => SurfaceExpr::Literal(SurfaceLiteral::Int(*n)) };

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
                let annotated_rhs = SurfaceExpr::Ann(Box::new(rhs), ty_expr).with_span(e.span());

                SurfaceExpr::Let(bind_name, Box::new(annotated_rhs), Box::new(then))
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

        let expr_ctor = ctor_ref_parser().spanned().map(SurfaceExpr::Con);
        let variable = var_ref_parser().spanned().map(SurfaceExpr::Var);

        let atom = choice((
            expr_int.spanned(),
            expr_let.spanned(),
            expr_ctor.spanned(),
            expr_let_anno.spanned(),
            expr_fun.spanned(),
            unit_expr.spanned(),
            variable.spanned(),
            expr_case.spanned(),
            expr_if.spanned(),
            expr_list,
            expr.nested_in(select_ref! {
                Token::Parens(ts) = e => ts.split_spanned(e.span())
            }),
        ))
        .boxed();

        let app = atom
            .clone()
            .then(atom.repeated().collect::<Vec<_>>())
            .map(|(head, rest)| {
                let mut out = head;
                for arg in rest {
                    let span = merge_spans(&out.span, &arg.span);
                    out = SurfaceExpr::App(Box::new(out), Box::new(arg)).with_span(span);
                }
                out
            })
            .boxed();

        let cons_infix = infix(right(0), just(Token::ConsSep), |lhs, _, rhs, e| {
            mk_cons_expr(lhs, rhs, e.span())
        });

        let less_infix = infix(none(2), just(Token::Less), |lhs, _, rhs, e| {
            SurfaceExpr::BinaryOperator(SurfaceBinOp::Less, Box::new(lhs), Box::new(rhs))
                .with_span(e.span())
        });

        let greater_infix = infix(none(2), just(Token::Greater), |lhs, _, rhs, e| {
            SurfaceExpr::BinaryOperator(SurfaceBinOp::Greater, Box::new(lhs), Box::new(rhs))
                .with_span(e.span())
        });

        let less_eq_infix = infix(none(2), just(Token::LessOrEqual), |lhs, _, rhs, e| {
            SurfaceExpr::BinaryOperator(SurfaceBinOp::LessOrEqual, Box::new(lhs), Box::new(rhs))
                .with_span(e.span())
        });

        let greater_eq_infix = infix(none(2), just(Token::GreaterOrEqual), |lhs, _, rhs, e| {
            SurfaceExpr::BinaryOperator(SurfaceBinOp::GreaterOrEqual, Box::new(lhs), Box::new(rhs))
                .with_span(e.span())
        });

        let eq_infix = infix(none(2), just(Token::Equal), |lhs, _, rhs, e| {
            SurfaceExpr::BinaryOperator(SurfaceBinOp::Equal, Box::new(lhs), Box::new(rhs))
                .with_span(e.span())
        });

        let add_infix = infix(left(3), just(Token::Plus), |lhs, _, rhs, e| {
            SurfaceExpr::BinaryOperator(SurfaceBinOp::Plus, Box::new(lhs), Box::new(rhs))
                .with_span(e.span())
        });

        let sub_infix = infix(left(3), just(Token::Minus), |lhs, _, rhs, e| {
            SurfaceExpr::BinaryOperator(SurfaceBinOp::Minus, Box::new(lhs), Box::new(rhs))
                .with_span(e.span())
        });

        let mul_infix = infix(left(4), just(Token::Multiply), |lhs, _, rhs, e| {
            SurfaceExpr::BinaryOperator(SurfaceBinOp::Mult, Box::new(lhs), Box::new(rhs))
                .with_span(e.span())
        });

        let div_infix = infix(left(4), just(Token::Divide), |lhs, _, rhs, e| {
            SurfaceExpr::BinaryOperator(SurfaceBinOp::Div, Box::new(lhs), Box::new(rhs))
                .with_span(e.span())
        });

        let neg_prefix = prefix(5, just(Token::Minus), |_, rhs, e| {
            SurfaceExpr::UnaryOperator(SurfaceUnOp::Neg, Box::new(rhs)).with_span(e.span())
        });

        app.pratt((
            neg_prefix,
            mul_infix,
            div_infix,
            add_infix,
            sub_infix,
            less_infix,
            greater_infix,
            less_eq_infix,
            greater_eq_infix,
            eq_infix,
            cons_infix,
        ))
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
        // class_definition_parser(),
        // instance_definition_parser(),
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

fn plain_ref<'src>(name: &'src str) -> NameRef<'src> {
    NameRef {
        segments: vec![name],
    }
}

fn plain_ref_spanned<'src>(name: &'src str, span: SimpleSpan) -> Spanned<NameRef<'src>> {
    plain_ref(name).with_span(span)
}

fn qref<'src>(segments: &[&'src str]) -> NameRef<'src> {
    NameRef {
        segments: segments.to_vec(),
    }
}

fn qref_spanned<'src>(segments: &[&'src str], span: SimpleSpan) -> Spanned<NameRef<'src>> {
    qref(segments).with_span(span)
}

fn is_ctor_like(name: &str) -> bool {
    name.chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
}

fn path_ref_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    NameRef<'src>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    let seg = choice((
        select_ref! { Token::Ident(x) => *x },
        select_ref! { Token::UpperIdent(x) => *x },
    ));

    seg.separated_by(just(Token::Dot))
        .at_least(1)
        .collect::<Vec<_>>()
        .map(|segments| NameRef { segments })
}

fn ctor_ref_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    NameRef<'src>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    path_ref_parser().try_map(|r, span| {
        if is_ctor_like(r.last()) {
            Ok(r)
        } else {
            Err(Rich::custom(span, "expected constructor-like name"))
        }
    })
}

fn var_ref_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    NameRef<'src>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    path_ref_parser().try_map(|r, span| {
        if is_ctor_like(r.last()) {
            Err(Rich::custom(span, "expected variable/function-like name"))
        } else {
            Ok(r)
        }
    })
}

fn module_name_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    ModuleName,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    path_ref_parser().map(|p| ModuleName(p.segments.into_iter().map(str::to_string).collect()))
}

fn import_item_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    String,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    choice((
        select_ref! { Token::Ident(x) => (*x).to_string() },
        select_ref! { Token::UpperIdent(x) => (*x).to_string() },
    ))
}

fn import_decl_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    ImportDecl,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    just(Token::Import)
        .ignore_then(module_name_parser())
        .then(
            just(Token::As)
                .ignore_then(select_ref! { Token::Ident(x) => (*x).to_string() })
                .or_not(),
        )
        .then(
            import_item_parser()
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .nested_in(select_ref! {
                    Token::Parens(ts) = e => ts.split_spanned(e.span())
                })
                .or_not(),
        )
        .map(|((module, alias_opt), items_opt)| ImportDecl {
            alias: alias_opt.unwrap_or_else(|| module.default_alias()),
            module,
            spec: match items_opt {
                None => ImportSpec::All,
                Some(items) => ImportSpec::Only(items),
            },
        })
}

pub fn imports_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    Vec<ImportDecl>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    import_decl_parser()
        .then_ignore(just(Token::Semicolon))
        .repeated()
        .collect::<Vec<_>>()
}

pub fn module_parser<'tokens, 'src: 'tokens>() -> impl Parser<
    'tokens,
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [Spanned<Token<'src>>]>,
    ParsedModule<'src>,
    extra::Err<Rich<'tokens, Token<'src>>>,
> {
    imports_parser()
        .then(program_parser())
        .map(|(imports, defs)| ParsedModule { imports, defs })
}
