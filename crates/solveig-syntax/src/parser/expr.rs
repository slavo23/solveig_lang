//! Expression parser for Solveig terms and do-notation blocks.
//! The syntax tree produced here is still unresolved and fixity-sensitive.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_expr(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::Expr, |this| {
            let start = this.current_start();

            let expr = match this.current_kind() {
                Token::Fun => this.parse_lambda()?,
                Token::Let => this.parse_let()?,
                Token::Do => this.parse_do()?,
                Token::If => this.parse_if()?,
                Token::Case => this.parse_case()?,
                _ => this.parse_op_chain()?,
            };

            if this.at(&Token::Colon) {
                this.advance();
                let ty = this.parse_type()?;
                let end = span_end(&ty);
                return Ok(sp(SurfaceExpr::Ann(Box::new(expr), ty), start, end));
            }

            Ok(expr)
        })
    }

    pub(super) fn parse_op_chain(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::OpChain, |this| {
            let first = this.parse_op_operand()?;
            let mut segments = Vec::new();

            while this.is_infix_operator() {
                let (op, start, end) = this.expect_operator_symbol(true)?;
                let rhs = this.parse_op_operand()?;
                segments.push(SurfaceOpSegment {
                    op: sp(op, start, end),
                    rhs,
                });
            }

            if segments.is_empty() && first.prefix_minuses.is_empty() {
                Ok(first.expr)
            } else {
                let start = first
                    .prefix_minuses
                    .first()
                    .map_or_else(|| span_start(&first.expr), |span| span.start as usize);
                let end = segments.last().map_or_else(
                    || span_end(&first.expr),
                    |segment| span_end(&segment.rhs.expr),
                );
                Ok(sp(
                    SurfaceExpr::OpSeq(Box::new(first), segments),
                    start,
                    end,
                ))
            }
        })
    }

    pub(super) fn parse_op_operand(&mut self) -> Result<SurfaceOpOperand, ParseError> {
        self.node(SyntaxKind::OpOperand, |this| {
            let mut prefix_minuses = Vec::new();
            while this.at(&Token::Minus) {
                let token = this.advance();
                prefix_minuses.push(mk_span(token.start, token.end));
            }

            let expr = this.parse_app()?;
            Ok(SurfaceOpOperand {
                prefix_minuses,
                expr,
            })
        })
    }

    pub(super) fn parse_app(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::AppExpr, |this| {
            let mut func = this.parse_atom()?;
            while this.is_atom_start() {
                let arg = this.parse_atom()?;
                let (start, end) = merge_span(&func, &arg);
                func = sp(SurfaceExpr::App(Box::new(func), Box::new(arg)), start, end);
            }
            Ok(func)
        })
    }

    pub(super) fn is_atom_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Int(_)
                | Token::Float(_)
                | Token::Char(_)
                | Token::Str(_)
                | Token::Ident(_)
                | Token::UpperIdent(_)
                | Token::LParen
                | Token::LBrack
                | Token::Hole
                | Token::Underscore
        )
    }

    pub(super) fn parse_atom(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::AtomExpr, |this| {
            let start = this.current_start();
            match this.current_kind() {
                Token::Int(value) => {
                    let value = *value;
                    let token = this.advance();
                    Ok(sp(
                        SurfaceExpr::Literal(SurfaceLiteral::Int(value)),
                        token.start,
                        token.end,
                    ))
                }
                Token::Float(value) => {
                    let value = *value;
                    let token = this.advance();
                    Ok(sp(
                        SurfaceExpr::Literal(SurfaceLiteral::Float(value)),
                        token.start,
                        token.end,
                    ))
                }
                Token::Char(value) => {
                    let value = *value;
                    let token = this.advance();
                    Ok(sp(
                        SurfaceExpr::Literal(SurfaceLiteral::Char(value)),
                        token.start,
                        token.end,
                    ))
                }
                Token::Str(value) => {
                    let value = value.clone();
                    let token = this.advance();
                    Ok(sp(
                        SurfaceExpr::Literal(SurfaceLiteral::Str(value)),
                        token.start,
                        token.end,
                    ))
                }
                Token::Hole | Token::Underscore => {
                    let token = this.advance();
                    Ok(sp(SurfaceExpr::Hole, token.start, token.end))
                }
                Token::Ident(_) | Token::UpperIdent(_) => {
                    let name_ref = this.parse_name_path()?;
                    let end = span_end(&name_ref);
                    let expr = if is_ctor_like(&name_ref.0.last_text(this.session)) {
                        SurfaceExpr::Con(name_ref)
                    } else {
                        SurfaceExpr::Var(name_ref)
                    };
                    Ok(sp(expr, start, end))
                }
                Token::LParen => {
                    if this.starts_parenthesized_operator_name() {
                        let (symbol, start, end) = this.parse_parenthesized_operator_name()?;
                        let name_ref = sp(
                            NameRef {
                                segments: vec![symbol],
                            },
                            start,
                            end,
                        );
                        return Ok(sp(SurfaceExpr::Var(name_ref), start, end));
                    }

                    this.advance();
                    if this.at(&Token::RParen) {
                        let end = this.advance().end;
                        return Ok(sp(SurfaceExpr::Unit, start, end));
                    }
                    let inner = this.parse_expr()?;
                    let end = this.expect(&Token::RParen)?.end;
                    Ok(sp(inner.0, start, end))
                }
                Token::LBrack => {
                    this.advance();
                    if this.at(&Token::RBrack) {
                        let end = this.advance().end;
                        return Ok(list_nil_expr(this.session, start, end));
                    }
                    let mut elems = vec![this.parse_expr()?];
                    while this.at(&Token::Comma) {
                        this.advance();
                        elems.push(this.parse_expr()?);
                    }
                    let end = this.expect(&Token::RBrack)?.end;
                    Ok(list_expr(this.session, elems, start, end))
                }
                _ => Err(this.error(format!(
                    "expected expression, got {}",
                    this.describe_current()
                ))),
            }
        })
    }

    pub(super) fn parse_lambda(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::LambdaExpr, |this| {
            let start = this.expect(&Token::Fun)?.start;
            let mut params: Vec<LambdaParam> = Vec::new();

            while !this.at(&Token::FatArrow) {
                if this.at(&Token::LParen) {
                    this.advance();
                    let (name, start, end) = this.expect_binder()?;
                    this.expect(&Token::Colon)?;
                    let ty = this.parse_type()?;
                    this.expect(&Token::RParen)?;
                    params.push((sp(name, start, end), Some(ty)));
                } else {
                    let (name, start, end) = this.expect_binder()?;
                    params.push((sp(name, start, end), None));
                }
            }

            this.expect(&Token::FatArrow)?;
            let body = this.parse_expr()?;
            let end = span_end(&body);
            Ok(sp(SurfaceExpr::Abs(params, Box::new(body)), start, end))
        })
    }

    pub(super) fn parse_let(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::LetExpr, |this| {
            let start = this.expect(&Token::Let)?.start;
            let (name, name_start, name_end) = this.expect_ident()?;
            this.expect(&Token::Eq)?;
            let binding = this.parse_expr()?;
            this.expect(&Token::In)?;
            let body = this.parse_expr()?;
            let end = span_end(&body);
            Ok(sp(
                SurfaceExpr::Let(
                    sp(name, name_start, name_end),
                    Box::new(binding),
                    Box::new(body),
                ),
                start,
                end,
            ))
        })
    }

    pub(super) fn parse_if(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::IfExpr, |this| {
            let start = this.expect(&Token::If)?.start;
            let cond = this.parse_expr()?;
            this.expect(&Token::Then)?;
            let then_branch = this.parse_expr()?;
            this.expect(&Token::Else)?;
            let else_branch = this.parse_expr()?;
            let end = span_end(&else_branch);
            Ok(sp(
                SurfaceExpr::IfThenElse(
                    Box::new(cond),
                    Box::new(then_branch),
                    Box::new(else_branch),
                ),
                start,
                end,
            ))
        })
    }

    pub(super) fn parse_do(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::DoExpr, |this| {
            let start = this.expect(&Token::Do)?.start;

            let implicit_layout = if this.at(&Token::VOpen) {
                this.advance();
                true
            } else {
                false
            };

            let mut statements = Vec::new();
            loop {
                this.skip_separators();

                if this.at(&Token::VClose) {
                    if implicit_layout {
                        this.advance();
                    }
                    break;
                }

                if this.at_eof() || (implicit_layout && this.is_case_block_follower()) {
                    break;
                }

                let stmt_start = this.current_start();
                let stmt = this.parse_do_statement()?;
                let stmt_end = match &stmt {
                    SurfaceDoStmt::Bind { expr, .. }
                    | SurfaceDoStmt::Let { expr, .. }
                    | SurfaceDoStmt::Expr(expr) => span_end(expr),
                };
                statements.push(sp(stmt, stmt_start, stmt_end));
            }

            if statements.is_empty() {
                return Err(this.error("do block must contain at least one statement"));
            }

            let last = statements
                .last()
                .expect("ICE: checked non-empty do block before reading final statement");
            if !matches!(last.0, SurfaceDoStmt::Expr(_)) {
                return Err(ParseError {
                    span: FileSpan::new(this.file, last.1),
                    message: "final do statement must be an expression".to_string(),
                });
            }

            let end = span_end(last);
            Ok(sp(SurfaceExpr::Do(statements), start, end))
        })
    }

    pub(super) fn parse_do_statement(&mut self) -> Result<SurfaceDoStmt, ParseError> {
        self.node(SyntaxKind::DoStmt, |this| {
            if this.at(&Token::Let) {
                this.advance();
                let (name, start, end) = this.expect_ident()?;
                this.expect(&Token::Eq)?;
                let expr = this.parse_expr()?;
                return Ok(SurfaceDoStmt::Let {
                    binder: sp(name, start, end),
                    expr,
                });
            }

            if matches!(this.current_kind(), Token::Ident(_) | Token::Underscore)
                && this
                    .peek_kind(1)
                    .is_some_and(|kind| matches!(kind, Token::BindArrow))
            {
                let (name, start, end) = this.expect_binder()?;
                this.expect(&Token::BindArrow)?;
                let expr = this.parse_expr()?;
                return Ok(SurfaceDoStmt::Bind {
                    binder: sp(name, start, end),
                    expr,
                });
            }

            Ok(SurfaceDoStmt::Expr(this.parse_expr()?))
        })
    }

    pub(super) fn parse_case(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::CaseExpr, |this| {
            let start = this.expect(&Token::Case)?.start;
            let scrutinee = this.parse_expr()?;
            this.expect(&Token::Of)?;

            let implicit_layout = if this.at(&Token::VOpen) {
                this.advance();
                true
            } else {
                false
            };

            let mut arms = Vec::new();
            loop {
                this.skip_separators();

                if this.at(&Token::VClose) {
                    if implicit_layout {
                        this.advance();
                    }
                    break;
                }

                if this.at_eof() || (implicit_layout && this.is_case_block_follower()) {
                    break;
                }

                let arm = this.node(SyntaxKind::CaseArm, |this| {
                    if this.at(&Token::Pipe) {
                        this.advance();
                    }

                    let start = this.current_start();
                    let pattern = this.parse_pattern()?;
                    this.expect(&Token::FatArrow)?;
                    let expr = this.parse_expr()?;
                    let end = span_end(&expr);
                    Ok(sp(SurfaceMatchArm { pattern, expr }, start, end))
                })?;
                arms.push(arm);
            }

            let end = arms.last().map_or_else(|| span_end(&scrutinee), span_end);
            Ok(sp(SurfaceExpr::Case(Box::new(scrutinee), arms), start, end))
        })
    }
}
