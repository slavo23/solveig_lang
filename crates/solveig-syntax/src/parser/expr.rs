//! Expression syntax parsing.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_expr(&mut self) -> Result<(), ParseError> {
        self.parse_expr_with_left_section(false)
    }

    pub(super) fn parse_expr_with_left_section(
        &mut self,
        allow_left_section: bool,
    ) -> Result<(), ParseError> {
        self.node(SyntaxKind::Expr, |parser| {
            match parser.current_kind() {
                Token::Fun => parser.parse_lambda()?,
                Token::Let => parser.parse_let()?,
                Token::Do => parser.parse_do()?,
                Token::If => parser.parse_if()?,
                Token::Case => parser.parse_case()?,
                _ => parser.parse_op_chain(allow_left_section)?,
            }

            if parser.at(&Token::Colon) {
                parser.advance();
                parser.parse_type()?;
            }

            Ok(())
        })
    }

    pub(super) fn parse_op_chain(&mut self, allow_left_section: bool) -> Result<(), ParseError> {
        self.node(SyntaxKind::OpChain, |parser| {
            parser.parse_op_operand()?;

            while parser.is_infix_operator() {
                parser.parse_infix_operator_span(true)?;
                if allow_left_section && parser.at(&Token::RParen) {
                    return Ok(());
                }
                parser.parse_op_operand()?;
            }

            Ok(())
        })
    }

    pub(super) fn parse_op_operand(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::OpOperand, |parser| {
            while parser.at(&Token::Minus) {
                parser.advance();
            }

            parser.parse_app()
        })
    }

    pub(super) fn parse_app(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::AppExpr, |parser| {
            parser.parse_atom()?;
            while parser.is_atom_start() {
                parser.parse_atom()?;
            }
            Ok(())
        })
    }

    pub(super) fn is_atom_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Int
                | Token::Float
                | Token::Char
                | Token::Str
                | Token::Ident
                | Token::UpperIdent
                | Token::LParen
                | Token::LBrack
                | Token::Hole
                | Token::Underscore
        )
    }

    pub(super) fn parse_atom(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::AtomExpr, |parser| match parser.current_kind() {
            Token::Int
            | Token::Float
            | Token::Char
            | Token::Str
            | Token::Hole
            | Token::Underscore => {
                parser.advance();
                Ok(())
            }
            Token::Ident | Token::UpperIdent => {
                parser.parse_name_path()?;
                Ok(())
            }
            Token::LParen => {
                if parser.tuple_constructor_arity().is_some() {
                    return parser.parse_tuple_constructor();
                }

                if parser.starts_parenthesized_operator_name() {
                    parser.parse_parenthesized_operator_name()?;
                    return Ok(());
                }

                if parser.starts_operator_right_section() {
                    return parser.parse_operator_right_section();
                }

                if parser.starts_operator_left_section() {
                    return parser.parse_operator_left_section();
                }

                let kind = if parser.parenthesized_has_top_level_comma() {
                    SyntaxKind::TupleExpr
                } else {
                    SyntaxKind::ParenExpr
                };
                parser.node(kind, |parser| {
                    parser.advance();
                    if parser.at(&Token::RParen) {
                        parser.advance();
                        return Ok(());
                    }
                    parser.parse_expr()?;
                    let mut arity = 1usize;
                    while parser.at(&Token::Comma) {
                        parser.advance();
                        arity += 1;
                        parser.check_tuple_arity(arity)?;
                        if parser.at(&Token::RParen) {
                            return Err(parser.error("tuple elements may not be omitted"));
                        }
                        parser.parse_expr()?;
                    }
                    parser.expect(&Token::RParen)?;
                    Ok(())
                })
            }
            Token::LBrack => parser.node(SyntaxKind::ListExpr, |parser| {
                parser.advance();
                if parser.at(&Token::RBrack) {
                    parser.advance();
                    return Ok(());
                }
                parser.parse_expr()?;
                while parser.at(&Token::Comma) {
                    parser.advance();
                    parser.parse_expr()?;
                }
                parser.expect(&Token::RBrack)?;
                Ok(())
            }),
            _ => Err(parser.error(format!(
                "expected expression, got {}",
                parser.describe_current()
            ))),
        })
    }

    pub(super) fn parse_operator_right_section(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::OperatorSection, |parser| {
            parser.expect(&Token::LParen)?;
            parser.parse_infix_operator_span(true)?;
            parser.parse_expr()?;
            parser.expect(&Token::RParen)?;
            Ok(())
        })
    }

    pub(super) fn parse_operator_left_section(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::OperatorSection, |parser| {
            parser.expect(&Token::LParen)?;
            parser.parse_expr_with_left_section(true)?;
            parser.expect(&Token::RParen)?;
            Ok(())
        })
    }

    pub(super) fn token_is_section_operator(token: &Token) -> bool {
        matches!(
            token,
            Token::Operator | Token::Minus | Token::Cons | Token::Dot
        )
    }

    pub(super) fn starts_operator_right_section(&self) -> bool {
        if !self.at(&Token::LParen) {
            return false;
        }

        match self.peek_kind(1) {
            Some(Token::Minus) => false,
            Some(Token::Backtick) => {
                matches!(self.peek_kind(2), Some(Token::Ident))
                    && matches!(self.peek_kind(3), Some(Token::Backtick))
                    && !matches!(self.peek_kind(4), Some(Token::RParen))
            }
            Some(token) if Self::token_is_section_operator(token) => {
                !matches!(self.peek_kind(2), Some(Token::RParen))
            }
            _ => false,
        }
    }

    pub(super) fn significant_after_index(&self, index: usize) -> Option<usize> {
        self.tokens
            .iter()
            .enumerate()
            .skip(index + 1)
            .find(|(_, token)| !token.kind.is_trivia())
            .map(|(index, _)| index)
    }

    pub(super) fn significant_before_index(&self, index: usize) -> Option<usize> {
        self.tokens_before(index)
            .iter()
            .enumerate()
            .rev()
            .find(|(_, token)| !token.kind.is_trivia())
            .map(|(index, _)| index)
    }

    pub(super) fn matching_rparen_index(&self, lparen_index: usize) -> Option<usize> {
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

    pub(super) fn trailing_backticked_operator_start(&self, close_index: usize) -> Option<usize> {
        let close_backtick = self.significant_before_index(close_index)?;
        if !matches!(self.token(close_backtick).kind, Token::Backtick) {
            return None;
        }

        let name = self.significant_before_index(close_backtick)?;
        if !matches!(self.token(name).kind, Token::Ident) {
            return None;
        }

        let open_backtick = self.significant_before_index(name)?;
        if matches!(self.token(open_backtick).kind, Token::Backtick) {
            Some(open_backtick)
        } else {
            None
        }
    }

    pub(super) fn trailing_operator_start(&self, close_index: usize) -> Option<usize> {
        let previous = self.significant_before_index(close_index)?;
        if Self::token_is_section_operator(&self.token(previous).kind) {
            Some(previous)
        } else {
            self.trailing_backticked_operator_start(close_index)
        }
    }

    pub(super) fn starts_operator_left_section(&self) -> bool {
        if !self.at(&Token::LParen) {
            return false;
        }

        let lparen_index = self.current_index();
        let close_index = match self.matching_rparen_index(lparen_index) {
            Some(close_index) => close_index,
            None => return false,
        };
        let first_inside_index = match self.significant_after_index(lparen_index) {
            Some(first_inside_index) => first_inside_index,
            None => return false,
        };
        let operator_start_index = match self.trailing_operator_start(close_index) {
            Some(operator_start_index) => operator_start_index,
            None => return false,
        };

        first_inside_index < operator_start_index
    }

    pub(super) fn parse_lambda(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::LambdaExpr, |parser| {
            parser.expect(&Token::Fun)?;

            while !parser.at(&Token::FatArrow) {
                parser.node(SyntaxKind::LambdaParam, |parser| {
                    if parser.parenthesized_has_top_level_colon() {
                        parser.expect(&Token::LParen)?;
                        parser.parse_pattern()?;
                        parser.expect(&Token::Colon)?;
                        parser.parse_type()?;
                        parser.expect(&Token::RParen)?;
                        Ok(())
                    } else {
                        parser.parse_pattern_arg()
                    }
                })?;
            }

            parser.expect(&Token::FatArrow)?;
            parser.parse_expr()
        })
    }

    pub(super) fn parse_let(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::LetExpr, |parser| {
            parser.expect(&Token::Let)?;
            parser.node(SyntaxKind::PatternBinder, |parser| parser.parse_pattern())?;
            parser.expect(&Token::Eq)?;
            parser.parse_expr()?;
            parser.expect(&Token::In)?;
            parser.parse_expr()
        })
    }

    pub(super) fn parse_if(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::IfExpr, |parser| {
            parser.expect(&Token::If)?;
            parser.parse_expr()?;
            parser.expect(&Token::Then)?;
            parser.parse_expr()?;
            parser.expect(&Token::Else)?;
            parser.parse_expr()
        })
    }

    pub(super) fn parse_do(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::DoExpr, |parser| {
            parser.expect(&Token::Do)?;

            let implicit_layout = if parser.at(&Token::VOpen) {
                parser.advance();
                true
            } else {
                false
            };

            let mut statement_count = 0usize;
            let mut final_statement = None;
            loop {
                parser.skip_separators();

                if parser.at(&Token::VClose) {
                    if implicit_layout {
                        parser.advance();
                    }
                    break;
                }

                if parser.at_eof() || (implicit_layout && parser.is_case_block_follower()) {
                    break;
                }

                final_statement = Some(parser.parse_do_statement()?);
                statement_count += 1;
            }

            if statement_count == 0 {
                return Err(parser.error("do block must contain at least one statement"));
            }

            if !final_statement.is_some_and(|statement| statement.kind == DoStatementKind::Expr) {
                let span = final_statement.map_or_else(
                    || parser.current_span(),
                    |statement| {
                        FileSpan::new(
                            parser.file,
                            mk_span(statement.start, statement.end.max(statement.start)),
                        )
                    },
                );
                return Err(ParseError {
                    span,
                    message: "final do statement must be an expression".to_string(),
                });
            }

            Ok(())
        })
    }

    pub(super) fn parse_do_statement(&mut self) -> Result<DoStatementFacts, ParseError> {
        self.node(SyntaxKind::DoStmt, |parser| {
            let statement_start = parser.current_start();

            if parser.at(&Token::Let) {
                parser.advance();
                parser.node(SyntaxKind::PatternBinder, |parser| parser.parse_pattern())?;
                parser.expect(&Token::Eq)?;
                parser.parse_expr()?;
                return Ok(DoStatementFacts {
                    kind: DoStatementKind::Let,
                    start: statement_start,
                    end: parser
                        .previous_consumed_significant_token()
                        .map_or(statement_start, PosToken::end),
                });
            }

            if parser.do_statement_has_top_level_bind_arrow() {
                parser.node(SyntaxKind::PatternBinder, |parser| parser.parse_pattern())?;
                parser.expect(&Token::BindArrow)?;
                parser.parse_expr()?;
                return Ok(DoStatementFacts {
                    kind: DoStatementKind::Bind,
                    start: statement_start,
                    end: parser
                        .previous_consumed_significant_token()
                        .map_or(statement_start, PosToken::end),
                });
            }

            parser.parse_expr()?;
            Ok(DoStatementFacts {
                kind: DoStatementKind::Expr,
                start: statement_start,
                end: parser
                    .previous_consumed_significant_token()
                    .map_or(statement_start, PosToken::end),
            })
        })
    }

    fn do_statement_has_top_level_bind_arrow(&self) -> bool {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;

        for token in self.tokens.iter().skip(self.current_index()) {
            if token.kind.is_trivia() {
                continue;
            }
            match token.kind {
                Token::LParen => paren_depth += 1,
                Token::RParen => paren_depth = paren_depth.saturating_sub(1),
                Token::LBrack => bracket_depth += 1,
                Token::RBrack => bracket_depth = bracket_depth.saturating_sub(1),
                Token::BindArrow if paren_depth == 0 && bracket_depth == 0 => return true,
                Token::Semicolon | Token::VSemi | Token::VClose
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    return false;
                }
                _ => {}
            }
        }

        false
    }

    pub(super) fn parse_case(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::CaseExpr, |parser| {
            parser.expect(&Token::Case)?;
            parser.parse_expr()?;
            parser.expect(&Token::Of)?;

            let implicit_layout = if parser.at(&Token::VOpen) {
                parser.advance();
                true
            } else {
                false
            };

            loop {
                parser.skip_separators();

                if parser.at(&Token::VClose) {
                    if implicit_layout {
                        parser.advance();
                    }
                    break;
                }

                if parser.at_eof() || (implicit_layout && parser.is_case_block_follower()) {
                    break;
                }

                parser.parse_case_arm()?;
            }

            Ok(())
        })
    }

    pub(super) fn parse_case_arm(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::CaseArm, |parser| {
            if parser.at(&Token::Pipe) {
                parser.advance();
            }

            parser.parse_pattern()?;
            parser.expect(&Token::FatArrow)?;
            parser.parse_expr()
        })
    }

    pub(super) fn is_case_block_follower(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::RParen
                | Token::RBrack
                | Token::Comma
                | Token::Then
                | Token::Else
                | Token::In
                | Token::BindArrow
                | Token::Semicolon
                | Token::VSemi
                | Token::VClose
                | Token::Eof
        )
    }
}
