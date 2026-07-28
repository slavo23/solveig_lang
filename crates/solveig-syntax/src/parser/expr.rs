//! Expression parser for Solveig terms and do-notation blocks.
//! The syntax tree produced here is still unresolved and fixity-sensitive.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_expr(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.current_start();

        let expr = match self.current_kind() {
            Token::Fun => return self.parse_lambda(),
            Token::Let => return self.parse_let(),
            Token::Do => return self.parse_do(),
            Token::If => return self.parse_if(),
            Token::Case => return self.parse_case(),
            _ => self.parse_op_chain()?,
        };

        if self.at(&Token::Colon) {
            self.advance();
            let ty = self.parse_type()?;
            let end = span_end(&ty);
            return Ok(sp(SurfaceExpr::Ann(Box::new(expr), ty), start, end));
        }

        Ok(expr)
    }

    pub(super) fn parse_op_chain(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let first = self.parse_op_operand()?;
        let mut segments = Vec::new();

        while self.is_infix_operator() {
            let (op, start, end) = self.expect_operator_symbol(true)?;
            let rhs = self.parse_op_operand()?;
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
    }

    pub(super) fn parse_op_operand(&mut self) -> Result<SurfaceOpOperand, ParseError> {
        let mut prefix_minuses = Vec::new();
        while self.at(&Token::Minus) {
            let token = self.advance();
            prefix_minuses.push(mk_span(token.start, token.end));
        }

        let expr = self.parse_app()?;
        Ok(SurfaceOpOperand {
            prefix_minuses,
            expr,
        })
    }

    pub(super) fn parse_app(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let mut func = self.parse_atom()?;
        while self.is_atom_start() {
            let arg = self.parse_atom()?;
            let (start, end) = merge_span(&func, &arg);
            func = sp(SurfaceExpr::App(Box::new(func), Box::new(arg)), start, end);
        }
        Ok(func)
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
        let start = self.current_start();
        match self.current_kind() {
            Token::Int(value) => {
                let value = *value;
                let token = self.advance();
                Ok(sp(
                    SurfaceExpr::Literal(SurfaceLiteral::Int(value)),
                    token.start,
                    token.end,
                ))
            }
            Token::Float(value) => {
                let value = *value;
                let token = self.advance();
                Ok(sp(
                    SurfaceExpr::Literal(SurfaceLiteral::Float(value)),
                    token.start,
                    token.end,
                ))
            }
            Token::Char(value) => {
                let value = *value;
                let token = self.advance();
                Ok(sp(
                    SurfaceExpr::Literal(SurfaceLiteral::Char(value)),
                    token.start,
                    token.end,
                ))
            }
            Token::Str(value) => {
                let value = value.clone();
                let token = self.advance();
                Ok(sp(
                    SurfaceExpr::Literal(SurfaceLiteral::Str(value)),
                    token.start,
                    token.end,
                ))
            }
            Token::Hole | Token::Underscore => {
                let token = self.advance();
                Ok(sp(SurfaceExpr::Hole, token.start, token.end))
            }
            Token::Ident(_) | Token::UpperIdent(_) => {
                let name_ref = self.parse_name_path()?;
                let end = span_end(&name_ref);
                let expr = if is_ctor_like(&name_ref.0.last_text(self.session)) {
                    SurfaceExpr::Con(name_ref)
                } else {
                    SurfaceExpr::Var(name_ref)
                };
                Ok(sp(expr, start, end))
            }
            Token::LParen => {
                if self.starts_parenthesized_operator_name() {
                    let (symbol, start, end) = self.parse_parenthesized_operator_name()?;
                    let name_ref = sp(
                        NameRef {
                            segments: vec![symbol],
                        },
                        start,
                        end,
                    );
                    return Ok(sp(SurfaceExpr::Var(name_ref), start, end));
                }

                self.advance();
                if self.at(&Token::RParen) {
                    let end = self.advance().end;
                    return Ok(sp(SurfaceExpr::Unit, start, end));
                }
                let inner = self.parse_expr()?;
                let end = self.expect(&Token::RParen)?.end;
                Ok(sp(inner.0, start, end))
            }
            Token::LBrack => {
                self.advance();
                if self.at(&Token::RBrack) {
                    let end = self.advance().end;
                    return Ok(list_nil_expr(self.session, start, end));
                }
                let mut elems = vec![self.parse_expr()?];
                while self.at(&Token::Comma) {
                    self.advance();
                    elems.push(self.parse_expr()?);
                }
                let end = self.expect(&Token::RBrack)?.end;
                Ok(list_expr(self.session, elems, start, end))
            }
            _ => Err(self.error(format!(
                "expected expression, got {}",
                self.describe_current()
            ))),
        }
    }

    pub(super) fn parse_lambda(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.expect(&Token::Fun)?.start;
        let mut params: Vec<LambdaParam> = Vec::new();

        while !self.at(&Token::FatArrow) {
            if self.at(&Token::LParen) {
                self.advance();
                let (name, start, end) = self.expect_binder()?;
                self.expect(&Token::Colon)?;
                let ty = self.parse_type()?;
                self.expect(&Token::RParen)?;
                params.push((sp(name, start, end), Some(ty)));
            } else {
                let (name, start, end) = self.expect_binder()?;
                params.push((sp(name, start, end), None));
            }
        }

        self.expect(&Token::FatArrow)?;
        let body = self.parse_expr()?;
        let end = span_end(&body);
        Ok(sp(SurfaceExpr::Abs(params, Box::new(body)), start, end))
    }

    pub(super) fn parse_let(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.expect(&Token::Let)?.start;
        let (name, name_start, name_end) = self.expect_ident()?;
        self.expect(&Token::Eq)?;
        let binding = self.parse_expr()?;
        self.expect(&Token::In)?;
        let body = self.parse_expr()?;
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
    }

    pub(super) fn parse_if(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.expect(&Token::If)?.start;
        let cond = self.parse_expr()?;
        self.expect(&Token::Then)?;
        let then_branch = self.parse_expr()?;
        self.expect(&Token::Else)?;
        let else_branch = self.parse_expr()?;
        let end = span_end(&else_branch);
        Ok(sp(
            SurfaceExpr::IfThenElse(Box::new(cond), Box::new(then_branch), Box::new(else_branch)),
            start,
            end,
        ))
    }

    pub(super) fn parse_do(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.expect(&Token::Do)?.start;

        let implicit_layout = if self.at(&Token::VOpen) {
            self.advance();
            true
        } else {
            false
        };

        let mut statements = Vec::new();
        loop {
            self.skip_separators();

            if self.at(&Token::VClose) {
                if implicit_layout {
                    self.advance();
                }
                break;
            }

            if self.at_eof() || (implicit_layout && self.is_case_block_follower()) {
                break;
            }

            let stmt_start = self.current_start();
            let stmt = self.parse_do_statement()?;
            let stmt_end = match &stmt {
                SurfaceDoStmt::Bind { expr, .. }
                | SurfaceDoStmt::Let { expr, .. }
                | SurfaceDoStmt::Expr(expr) => span_end(expr),
            };
            statements.push(sp(stmt, stmt_start, stmt_end));
        }

        if statements.is_empty() {
            return Err(self.error("do block must contain at least one statement"));
        }

        let last = statements
            .last()
            .expect("ICE: checked non-empty do block before reading final statement");
        if !matches!(last.0, SurfaceDoStmt::Expr(_)) {
            return Err(ParseError {
                span: FileSpan::new(self.file, last.1),
                message: "final do statement must be an expression".to_string(),
            });
        }

        let end = span_end(last);
        Ok(sp(SurfaceExpr::Do(statements), start, end))
    }

    pub(super) fn parse_do_statement(&mut self) -> Result<SurfaceDoStmt, ParseError> {
        if self.at(&Token::Let) {
            self.advance();
            let (name, start, end) = self.expect_ident()?;
            self.expect(&Token::Eq)?;
            let expr = self.parse_expr()?;
            return Ok(SurfaceDoStmt::Let {
                binder: sp(name, start, end),
                expr,
            });
        }

        if matches!(self.current_kind(), Token::Ident(_) | Token::Underscore)
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| matches!(token.kind, Token::BindArrow))
        {
            let (name, start, end) = self.expect_binder()?;
            self.expect(&Token::BindArrow)?;
            let expr = self.parse_expr()?;
            return Ok(SurfaceDoStmt::Bind {
                binder: sp(name, start, end),
                expr,
            });
        }

        Ok(SurfaceDoStmt::Expr(self.parse_expr()?))
    }

    pub(super) fn parse_case(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        let start = self.expect(&Token::Case)?.start;
        let scrutinee = self.parse_expr()?;
        self.expect(&Token::Of)?;

        let implicit_layout = if self.at(&Token::VOpen) {
            self.advance();
            true
        } else {
            false
        };

        let mut arms = Vec::new();
        loop {
            self.skip_separators();

            if self.at(&Token::VClose) {
                if implicit_layout {
                    self.advance();
                }
                break;
            }

            if self.at_eof() || (implicit_layout && self.is_case_block_follower()) {
                break;
            }

            if self.at(&Token::Pipe) {
                self.advance();
            }

            let start = self.current_start();
            let pattern = self.parse_pattern()?;
            self.expect(&Token::FatArrow)?;
            let expr = self.parse_expr()?;
            let end = span_end(&expr);
            arms.push(sp(SurfaceMatchArm { pattern, expr }, start, end));
        }

        let end = arms.last().map_or_else(|| span_end(&scrutinee), span_end);
        Ok(sp(SurfaceExpr::Case(Box::new(scrutinee), arms), start, end))
    }
}
