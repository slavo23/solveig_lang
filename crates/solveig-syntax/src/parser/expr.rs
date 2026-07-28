//! Expression parser for Solveig terms and do-notation blocks.
//! The syntax tree produced here is still unresolved and fixity-sensitive.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_expr(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.parse_expr_with_left_section(false)
    }

    fn parse_expr_with_left_section(
        &mut self,
        allow_left_section: bool,
    ) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::Expr, |this| {
            let start = this.current_start();

            let expr = match this.current_kind() {
                Token::Fun => this.parse_lambda()?,
                Token::Let => this.parse_let()?,
                Token::Do => this.parse_do()?,
                Token::If => this.parse_if()?,
                Token::Case => this.parse_case()?,
                _ => this.parse_op_chain(allow_left_section)?,
            };

            if this.at(&Token::Colon) {
                this.advance();
                let ty = this.parse_type()?;
                let end = span_end(&ty);
                return Ok(this.sp(SurfaceExpr::Ann(Box::new(expr), ty), start, end));
            }

            Ok(expr)
        })
    }

    fn op_chain_start(first: &SurfaceOpOperand) -> usize {
        first
            .prefix_minuses
            .first()
            .map_or_else(|| span_start(&first.expr), |span| span.start as usize)
    }

    fn op_chain_end(first: &SurfaceOpOperand, segments: &[SurfaceOpSegment]) -> usize {
        segments.last().map_or_else(
            || span_end(&first.expr),
            |segment| span_end(&segment.rhs.expr),
        )
    }

    fn op_chain_expr(
        &self,
        first: SurfaceOpOperand,
        segments: Vec<SurfaceOpSegment>,
    ) -> S<SurfaceExpr> {
        if segments.is_empty() && first.prefix_minuses.is_empty() {
            first.expr
        } else {
            let start = Self::op_chain_start(&first);
            let end = Self::op_chain_end(&first, &segments);
            self.sp(SurfaceExpr::OpSeq(Box::new(first), segments), start, end)
        }
    }

    pub(super) fn parse_op_chain(
        &mut self,
        allow_left_section: bool,
    ) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::OpChain, |this| {
            let first = this.parse_op_operand()?;
            let mut segments = Vec::new();

            while this.is_infix_operator() {
                let (op, start, end) = this.parse_infix_operator_symbol(true)?;
                if allow_left_section && this.at(&Token::RParen) {
                    let lhs = this.op_chain_expr(first, segments);
                    let op = this.sp(op, start, end);
                    let section_start = span_start(&lhs);
                    return Ok(this.sp(
                        SurfaceExpr::OperatorLeftSection(Box::new(lhs), op),
                        section_start,
                        end,
                    ));
                }

                let rhs = this.parse_op_operand()?;
                let cst_id = this.current_cst_id();
                segments.push(SurfaceOpSegment {
                    op: this.sp(op, start, end),
                    rhs,
                    cst_id,
                });
            }

            Ok(this.op_chain_expr(first, segments))
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
                cst_id: this.current_cst_id(),
            })
        })
    }

    pub(super) fn parse_app(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::AppExpr, |this| {
            let mut func = this.parse_atom()?;
            while this.is_atom_start() {
                let arg = this.parse_atom()?;
                let (start, end) = merge_span(&func, &arg);
                func = this.sp(SurfaceExpr::App(Box::new(func), Box::new(arg)), start, end);
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
                    Ok(this.sp(
                        SurfaceExpr::Literal(SurfaceLiteral::Int(value)),
                        token.start,
                        token.end,
                    ))
                }
                Token::Float(value) => {
                    let value = *value;
                    let token = this.advance();
                    Ok(this.sp(
                        SurfaceExpr::Literal(SurfaceLiteral::Float(value)),
                        token.start,
                        token.end,
                    ))
                }
                Token::Char(value) => {
                    let value = *value;
                    let token = this.advance();
                    Ok(this.sp(
                        SurfaceExpr::Literal(SurfaceLiteral::Char(value)),
                        token.start,
                        token.end,
                    ))
                }
                Token::Str(value) => {
                    let value = value.clone();
                    let token = this.advance();
                    Ok(this.sp(
                        SurfaceExpr::Literal(SurfaceLiteral::Str(value)),
                        token.start,
                        token.end,
                    ))
                }
                Token::Hole | Token::Underscore => {
                    let token = this.advance();
                    Ok(this.sp(SurfaceExpr::Hole, token.start, token.end))
                }
                Token::Ident(_) | Token::UpperIdent(_) => {
                    let name_ref = this.parse_name_path()?;
                    let end = span_end(&name_ref);
                    let expr = if is_ctor_like(&name_ref.0.last_text(this.session)) {
                        SurfaceExpr::Con(name_ref)
                    } else {
                        SurfaceExpr::Var(name_ref)
                    };
                    Ok(this.sp(expr, start, end))
                }
                Token::LParen => {
                    if this.starts_parenthesized_operator_name() {
                        let (symbol, start, end) = this.parse_parenthesized_operator_name()?;
                        let name_ref = this.sp(
                            NameRef {
                                segments: vec![symbol],
                            },
                            start,
                            end,
                        );
                        return Ok(this.sp(SurfaceExpr::Var(name_ref), start, end));
                    }

                    if this.starts_operator_right_section() {
                        return this.parse_operator_right_section();
                    }

                    if this.starts_operator_left_section() {
                        return this.parse_operator_left_section();
                    }

                    this.advance();
                    if this.at(&Token::RParen) {
                        let end = this.advance().end;
                        return Ok(this.sp(SurfaceExpr::Unit, start, end));
                    }
                    let inner = this.parse_expr()?;
                    let end = this.expect(&Token::RParen)?.end;
                    Ok(this.sp(inner.0, start, end))
                }
                Token::LBrack => {
                    this.advance();
                    if this.at(&Token::RBrack) {
                        let end = this.advance().end;
                        return Ok(list_nil_expr(
                            this.session,
                            start,
                            end,
                            this.current_cst_id(),
                        ));
                    }
                    let mut elems = vec![this.parse_expr()?];
                    while this.at(&Token::Comma) {
                        this.advance();
                        elems.push(this.parse_expr()?);
                    }
                    let end = this.expect(&Token::RBrack)?.end;
                    Ok(list_expr(
                        this.session,
                        elems,
                        start,
                        end,
                        this.current_cst_id(),
                    ))
                }
                _ => Err(this.error(format!(
                    "expected expression, got {}",
                    this.describe_current()
                ))),
            }
        })
    }

    fn parse_operator_right_section(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::OperatorSection, |this| {
            let start = this.expect(&Token::LParen)?.start;
            let (op, op_start, op_end) = this.parse_infix_operator_symbol(true)?;
            let rhs = this.parse_expr()?;
            let end = this.expect(&Token::RParen)?.end;
            let op = this.sp(op, op_start, op_end);
            Ok(this.sp(
                SurfaceExpr::OperatorRightSection(op, Box::new(rhs)),
                start,
                end,
            ))
        })
    }

    fn parse_operator_left_section(&mut self) -> Result<S<SurfaceExpr>, ParseError> {
        self.node(SyntaxKind::OperatorSection, |this| {
            let start = this.expect(&Token::LParen)?.start;
            let section = this.parse_expr_with_left_section(true)?;
            let end = this.expect(&Token::RParen)?.end;

            match section.0 {
                section @ SurfaceExpr::OperatorLeftSection(_, _) => {
                    Ok(this.sp(section, start, end))
                }
                _ => Err(this.error("expected operator section")),
            }
        })
    }

    fn token_is_section_operator(token: &Token) -> bool {
        matches!(
            token,
            Token::Operator(_)
                | Token::Plus
                | Token::Minus
                | Token::Star
                | Token::Slash
                | Token::Percent
                | Token::EqEq
                | Token::Less
                | Token::Greater
                | Token::LessEq
                | Token::GreaterEq
                | Token::Cons
                | Token::Dot
        )
    }

    fn starts_operator_right_section(&self) -> bool {
        if !self.at(&Token::LParen) {
            return false;
        }

        match self.peek_kind(1) {
            Some(Token::Minus) => false,
            Some(Token::Backtick) => {
                matches!(self.peek_kind(2), Some(Token::Ident(_)))
                    && matches!(self.peek_kind(3), Some(Token::Backtick))
                    && !matches!(self.peek_kind(4), Some(Token::RParen))
            }
            Some(token) if Self::token_is_section_operator(token) => {
                !matches!(self.peek_kind(2), Some(Token::RParen))
            }
            _ => false,
        }
    }

    fn significant_after_index(&self, index: usize) -> Option<usize> {
        self.tokens
            .iter()
            .enumerate()
            .skip(index + 1)
            .find(|(_, token)| !token.kind.is_trivia())
            .map(|(index, _)| index)
    }

    fn significant_before_index(&self, index: usize) -> Option<usize> {
        self.tokens[..index.min(self.tokens.len())]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, token)| !token.kind.is_trivia())
            .map(|(index, _)| index)
    }

    fn matching_rparen_index(&self, lparen_index: usize) -> Option<usize> {
        let mut depth = 0usize;

        for (index, token) in self.tokens.iter().enumerate().skip(lparen_index) {
            if token.kind.is_trivia() {
                continue;
            }

            match token.kind {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }

        None
    }

    fn trailing_backticked_operator_start(&self, close_index: usize) -> Option<usize> {
        let close_backtick = self.significant_before_index(close_index)?;
        if !matches!(self.tokens[close_backtick].kind, Token::Backtick) {
            return None;
        }

        let name = self.significant_before_index(close_backtick)?;
        if !matches!(self.tokens[name].kind, Token::Ident(_)) {
            return None;
        }

        let open_backtick = self.significant_before_index(name)?;
        if matches!(self.tokens[open_backtick].kind, Token::Backtick) {
            Some(open_backtick)
        } else {
            None
        }
    }

    fn trailing_operator_start(&self, close_index: usize) -> Option<usize> {
        let previous = self.significant_before_index(close_index)?;
        if Self::token_is_section_operator(&self.tokens[previous].kind) {
            Some(previous)
        } else {
            self.trailing_backticked_operator_start(close_index)
        }
    }

    fn starts_operator_left_section(&self) -> bool {
        if !self.at(&Token::LParen) {
            return false;
        }

        let lparen = self.current_index();
        let close = match self.matching_rparen_index(lparen) {
            Some(close) => close,
            None => return false,
        };
        let first = match self.significant_after_index(lparen) {
            Some(first) => first,
            None => return false,
        };
        let op_start = match self.trailing_operator_start(close) {
            Some(op_start) => op_start,
            None => return false,
        };

        first < op_start
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
                    params.push((this.sp(name, start, end), Some(ty)));
                } else {
                    let (name, start, end) = this.expect_binder()?;
                    params.push((this.sp(name, start, end), None));
                }
            }

            this.expect(&Token::FatArrow)?;
            let body = this.parse_expr()?;
            let end = span_end(&body);
            Ok(this.sp(SurfaceExpr::Abs(params, Box::new(body)), start, end))
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
            Ok(this.sp(
                SurfaceExpr::Let(
                    this.sp(name, name_start, name_end),
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
            Ok(this.sp(
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
                statements.push(sp(stmt, stmt_start, stmt_end, this.completed_cst_id()));
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
            Ok(this.sp(SurfaceExpr::Do(statements), start, end))
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
                    binder: this.sp(name, start, end),
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
                    binder: this.sp(name, start, end),
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
                    Ok(this.sp(SurfaceMatchArm { pattern, expr }, start, end))
                })?;
                arms.push(arm);
            }

            let end = arms.last().map_or_else(|| span_end(&scrutinee), span_end);
            Ok(this.sp(SurfaceExpr::Case(Box::new(scrutinee), arms), start, end))
        })
    }
}
