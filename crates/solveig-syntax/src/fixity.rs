use solveig_base::{Span, Symbol};

use crate::ast::{
    Assoc, Fixity, Spanned, SurfaceExpr, SurfaceOpOperand, SurfaceOpSegment, SurfaceUnOp,
};

fn mk_span(start: usize, end: usize) -> Span {
    Span::from_usize_range(start..end)
}

fn span_start<T>(spanned: &Spanned<T>) -> usize {
    spanned.1.start as usize
}

fn span_end<T>(spanned: &Spanned<T>) -> usize {
    spanned.1.end as usize
}

fn sp<T>(value: T, start: usize, end: usize) -> Spanned<T> {
    (value, mk_span(start, end))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixityError {
    UnknownOperator {
        span: Span,
        op: Symbol,
    },
    Conflict {
        span: Span,
        left: Symbol,
        right: Symbol,
    },
    IllegalUnaryMinus {
        span: Span,
    },
    ExpectedExpression {
        span: Span,
    },
}

#[derive(Debug, Clone)]
struct Op {
    symbol: Spanned<Symbol>,
    fixity: Fixity,
}

#[derive(Debug, Clone)]
struct ActiveOp {
    symbol: Option<Spanned<Symbol>>,
    fixity: Fixity,
    sentinel: bool,
}

impl ActiveOp {
    fn sentinel() -> Self {
        Self {
            symbol: None,
            fixity: Fixity {
                prec: 0,
                assoc: Assoc::None,
            },
            sentinel: true,
        }
    }

    fn prefix_minus() -> Self {
        Self {
            symbol: None,
            fixity: Fixity {
                prec: 6,
                assoc: Assoc::Left,
            },
            sentinel: false,
        }
    }

    fn prec(&self) -> i16 {
        if self.sentinel {
            -1
        } else {
            i16::from(self.fixity.prec)
        }
    }
}

impl From<&Op> for ActiveOp {
    fn from(op: &Op) -> Self {
        Self {
            symbol: Some(op.symbol),
            fixity: op.fixity,
            sentinel: false,
        }
    }
}

#[derive(Debug, Clone)]
enum Token {
    Expr(Spanned<SurfaceExpr>),
    Op(Op),
    Neg(Span),
}

fn push_operand_tokens(tokens: &mut Vec<Token>, operand: &SurfaceOpOperand) {
    for span in &operand.prefix_minuses {
        tokens.push(Token::Neg(*span));
    }
    tokens.push(Token::Expr(operand.expr.clone()));
}

fn op_conflict(op1: &ActiveOp, op2: &Op) -> Option<FixityError> {
    if op1.sentinel || op1.prec() != i16::from(op2.fixity.prec) {
        return None;
    }

    let assoc1 = op1.fixity.assoc;
    let assoc2 = op2.fixity.assoc;
    if assoc1 == assoc2 && assoc1 != Assoc::None {
        return None;
    }

    Some(FixityError::Conflict {
        span: op2.symbol.1,
        left: op1.symbol.map(|symbol| symbol.0).unwrap_or(op2.symbol.0),
        right: op2.symbol.0,
    })
}

fn make_unary_minus(minus_span: Span, rhs: Spanned<SurfaceExpr>) -> Spanned<SurfaceExpr> {
    let start = minus_span.start as usize;
    let end = span_end(&rhs);
    sp(
        SurfaceExpr::UnaryOperator(SurfaceUnOp::Neg, Box::new(rhs)),
        start,
        end,
    )
}

fn make_operator_apply(
    lhs: Spanned<SurfaceExpr>,
    op: Spanned<Symbol>,
    rhs: Spanned<SurfaceExpr>,
) -> Spanned<SurfaceExpr> {
    let start = span_start(&lhs);
    let end = span_end(&rhs);
    sp(
        SurfaceExpr::OperatorApply(Box::new(lhs), op, Box::new(rhs)),
        start,
        end,
    )
}

// This follows the Haskell 2010 Report, section 10.6:
// https://www.haskell.org/onlinereport/haskell2010/haskellch10.html
// Prefix `-` is represented explicitly as `Neg` tokens so resolution can
// happen after names and imported fixities are known.
fn parse_neg(
    op1: &ActiveOp,
    tokens: &[Token],
    pos: usize,
) -> Result<(Spanned<SurfaceExpr>, usize), FixityError> {
    match tokens.get(pos) {
        Some(Token::Expr(expr)) => parse(op1, expr.clone(), tokens, pos + 1),
        Some(Token::Neg(span)) => {
            if op1.prec() >= 6 {
                return Err(FixityError::IllegalUnaryMinus { span: *span });
            }

            let (rhs, next) = parse_neg(&ActiveOp::prefix_minus(), tokens, pos + 1)?;
            parse(op1, make_unary_minus(*span, rhs), tokens, next)
        }
        Some(Token::Op(op)) => Err(FixityError::ExpectedExpression { span: op.symbol.1 }),
        None => Err(FixityError::ExpectedExpression {
            span: Span::from_usize_range(0..0),
        }),
    }
}

fn parse(
    op1: &ActiveOp,
    e1: Spanned<SurfaceExpr>,
    tokens: &[Token],
    pos: usize,
) -> Result<(Spanned<SurfaceExpr>, usize), FixityError> {
    let Some(Token::Op(op2)) = tokens.get(pos) else {
        return Ok((e1, pos));
    };

    if let Some(error) = op_conflict(op1, op2) {
        return Err(error);
    }

    let prec1 = op1.prec();
    let prec2 = i16::from(op2.fixity.prec);
    if prec1 > prec2 || (prec1 == prec2 && op1.fixity.assoc == Assoc::Left) {
        return Ok((e1, pos));
    }

    let (rhs, next) = parse_neg(&ActiveOp::from(op2), tokens, pos + 1)?;
    let applied = make_operator_apply(e1, op2.symbol, rhs);
    parse(op1, applied, tokens, next)
}

pub fn resolve_op_seq<F>(
    first: &SurfaceOpOperand,
    segments: &[SurfaceOpSegment],
    mut lookup: F,
) -> Result<Spanned<SurfaceExpr>, FixityError>
where
    F: FnMut(Spanned<Symbol>) -> Result<Fixity, FixityError>,
{
    let mut tokens = Vec::with_capacity(segments.len().saturating_mul(2) + 1);
    push_operand_tokens(&mut tokens, first);

    for segment in segments {
        let fixity = lookup(segment.op)?;
        tokens.push(Token::Op(Op {
            symbol: segment.op,
            fixity,
        }));
        push_operand_tokens(&mut tokens, &segment.rhs);
    }

    let (expr, next) = parse_neg(&ActiveOp::sentinel(), &tokens, 0)?;
    if next == tokens.len() {
        Ok(expr)
    } else {
        match &tokens[next] {
            Token::Expr(expr) => Err(FixityError::ExpectedExpression { span: expr.1 }),
            Token::Op(op) => Err(FixityError::ExpectedExpression { span: op.symbol.1 }),
            Token::Neg(span) => Err(FixityError::IllegalUnaryMinus { span: *span }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use solveig_base::{Session, Span};

    use super::*;
    use crate::ast::{NameRef, SurfaceExpr};

    fn mk_span(start: usize, end: usize) -> Span {
        Span::from_usize_range(start..end)
    }

    fn sp<T>(value: T, start: usize, end: usize) -> Spanned<T> {
        (value, mk_span(start, end))
    }

    fn var(session: &Session, name: &str, start: usize) -> Spanned<SurfaceExpr> {
        let symbol = session.intern(name);
        sp(
            SurfaceExpr::Var(sp(
                NameRef {
                    segments: vec![symbol],
                },
                start,
                start + name.len(),
            )),
            start,
            start + name.len(),
        )
    }

    fn operand(expr: Spanned<SurfaceExpr>) -> SurfaceOpOperand {
        SurfaceOpOperand {
            prefix_minuses: Vec::new(),
            expr,
        }
    }

    fn segment(
        session: &Session,
        op: &str,
        rhs: SurfaceOpOperand,
        start: usize,
    ) -> SurfaceOpSegment {
        SurfaceOpSegment {
            op: sp(session.intern(op), start, start + op.len()),
            rhs,
        }
    }

    fn render(session: &Session, expr: &Spanned<SurfaceExpr>) -> String {
        match &expr.0 {
            SurfaceExpr::Var(name) => session.str(name.0.last()).to_string(),
            SurfaceExpr::OperatorApply(lhs, op, rhs) => {
                format!(
                    "({} {} {})",
                    render(session, lhs),
                    session.str(op.0),
                    render(session, rhs)
                )
            }
            SurfaceExpr::UnaryOperator(SurfaceUnOp::Neg, rhs) => {
                format!("(-{})", render(session, rhs))
            }
            other => panic!("unexpected expression in fixity test: {other:?}"),
        }
    }

    fn lookup<'a>(
        session: &'a Session,
        map: &'a HashMap<&'static str, Fixity>,
    ) -> impl FnMut(Spanned<Symbol>) -> Result<Fixity, FixityError> + 'a {
        move |op| {
            map.get(session.str(op.0))
                .copied()
                .ok_or(FixityError::UnknownOperator {
                    span: op.1,
                    op: op.0,
                })
        }
    }

    #[test]
    fn left_associative_same_precedence_groups_left() {
        let session = Session::default();
        let mut fixities = HashMap::new();
        fixities.insert(
            "-",
            Fixity {
                prec: 6,
                assoc: Assoc::Left,
            },
        );

        let expr = resolve_op_seq(
            &operand(var(&session, "a", 0)),
            &[
                segment(&session, "-", operand(var(&session, "b", 4)), 2),
                segment(&session, "-", operand(var(&session, "c", 8)), 6),
            ],
            lookup(&session, &fixities),
        )
        .expect("left associative expression should resolve");

        assert_eq!(render(&session, &expr), "((a - b) - c)");
    }

    #[test]
    fn right_associative_same_precedence_groups_right() {
        let session = Session::default();
        let mut fixities = HashMap::new();
        fixities.insert(
            "^^",
            Fixity {
                prec: 8,
                assoc: Assoc::Right,
            },
        );

        let expr = resolve_op_seq(
            &operand(var(&session, "a", 0)),
            &[
                segment(&session, "^^", operand(var(&session, "b", 5)), 2),
                segment(&session, "^^", operand(var(&session, "c", 10)), 7),
            ],
            lookup(&session, &fixities),
        )
        .expect("right associative expression should resolve");

        assert_eq!(render(&session, &expr), "(a ^^ (b ^^ c))");
    }

    #[test]
    fn mixed_same_precedence_assoc_is_rejected() {
        let session = Session::default();
        let mut fixities = HashMap::new();
        fixities.insert(
            "<+>",
            Fixity {
                prec: 6,
                assoc: Assoc::Left,
            },
        );
        fixities.insert(
            "<^>",
            Fixity {
                prec: 6,
                assoc: Assoc::Right,
            },
        );

        let error = resolve_op_seq(
            &operand(var(&session, "a", 0)),
            &[
                segment(&session, "<+>", operand(var(&session, "b", 6)), 2),
                segment(&session, "<^>", operand(var(&session, "c", 12)), 8),
            ],
            lookup(&session, &fixities),
        )
        .expect_err("mixed associativity should fail");

        assert!(matches!(error, FixityError::Conflict { .. }));
    }

    #[test]
    fn prefix_minus_follows_haskell_precedence_rule() {
        let session = Session::default();
        let mut fixities = HashMap::new();
        fixities.insert(
            "*",
            Fixity {
                prec: 7,
                assoc: Assoc::Left,
            },
        );
        fixities.insert(
            "+",
            Fixity {
                prec: 6,
                assoc: Assoc::Left,
            },
        );

        let first = SurfaceOpOperand {
            prefix_minuses: vec![mk_span(0, 1)],
            expr: var(&session, "a", 1),
        };
        let expr = resolve_op_seq(
            &first,
            &[segment(&session, "*", operand(var(&session, "b", 5)), 3)],
            lookup(&session, &fixities),
        )
        .expect("prefix minus should resolve across higher precedence operator");

        assert_eq!(render(&session, &expr), "(-(a * b))");

        let first = SurfaceOpOperand {
            prefix_minuses: vec![mk_span(0, 1)],
            expr: var(&session, "a", 1),
        };
        let expr = resolve_op_seq(
            &first,
            &[segment(&session, "+", operand(var(&session, "b", 5)), 3)],
            lookup(&session, &fixities),
        )
        .expect("prefix minus should bind before same-precedence left operators");

        assert_eq!(render(&session, &expr), "((-a) + b)");

        let illegal_rhs = resolve_op_seq(
            &operand(var(&session, "a", 0)),
            &[SurfaceOpSegment {
                op: sp(session.intern("+"), 2, 3),
                rhs: SurfaceOpOperand {
                    prefix_minuses: vec![mk_span(4, 5)],
                    expr: var(&session, "b", 5),
                },
            }],
            lookup(&session, &fixities),
        );

        assert!(matches!(
            illegal_rhs,
            Err(FixityError::IllegalUnaryMinus { .. })
        ));
    }
}
