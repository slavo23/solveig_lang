use super::*;

impl Parser<'_> {
    pub(super) fn parse_pattern(&mut self) -> Result<S<SurfacePattern>, ParseError> {
        let head = self.parse_pattern_head()?;
        if self.at(&Token::Cons) {
            self.advance();
            let tail = self.parse_pattern()?;
            Ok(cons_pattern(self.session, head, tail))
        } else {
            Ok(head)
        }
    }

    pub(super) fn parse_pattern_head(&mut self) -> Result<S<SurfacePattern>, ParseError> {
        if matches!(self.current_kind(), Token::UpperIdent(_)) {
            let start = self.current_start();
            let name_ref = self.parse_name_path()?;
            let mut args = Vec::new();
            while self.is_pattern_arg_start() {
                args.push(self.parse_pattern_arg()?);
            }
            let end = args
                .last()
                .map(span_end)
                .unwrap_or_else(|| span_end(&name_ref));
            Ok(sp(SurfacePattern::Con(name_ref, args), start, end))
        } else {
            self.parse_pattern_arg()
        }
    }

    pub(super) fn parse_pattern_arg(&mut self) -> Result<S<SurfacePattern>, ParseError> {
        let start = self.current_start();

        match self.current_kind() {
            Token::Int(value) => {
                let value = *value;
                let token = self.advance();
                Ok(int_pattern(value, token.start, token.end))
            }

            Token::Float(value) => {
                let value = *value;
                let token = self.advance();
                Ok(literal_pattern(
                    SurfaceLiteral::Float(value),
                    token.start,
                    token.end,
                ))
            }

            Token::Char(value) => {
                let value = *value;
                let token = self.advance();
                Ok(literal_pattern(
                    SurfaceLiteral::Char(value),
                    token.start,
                    token.end,
                ))
            }

            Token::Str(value) => {
                let value = value.clone();
                let token = self.advance();
                Ok(literal_pattern(
                    SurfaceLiteral::Str(value),
                    token.start,
                    token.end,
                ))
            }

            Token::Minus => {
                // fix mutable borrow later
                let minus = self.advance().clone();
                match self.current_kind() {
                    Token::Int(value) => {
                        let value = value
                            .checked_neg()
                            .ok_or_else(|| self.error("integer literal pattern is too small"))?;

                        let int_token = self.advance();
                        Ok(int_pattern(value, minus.start, int_token.end))
                    }

                    Token::Float(value) => {
                        let value = -*value;
                        let float_token = self.advance();
                        Ok(literal_pattern(
                            SurfaceLiteral::Float(value),
                            minus.start,
                            float_token.end,
                        ))
                    }

                    _ => Err(self.error(format!(
                        "expected numeric literal after `-` in pattern, got {}",
                        self.describe_current()
                    ))),
                }
            }

            Token::Underscore => {
                let token = self.advance();
                Ok(sp(SurfacePattern::Wild, token.start, token.end))
            }

            Token::Ident(_) => {
                let name_ref = self.parse_name_path()?;
                let end = span_end(&name_ref);

                if name_ref.0.segments.len() == 1 {
                    Ok(sp(SurfacePattern::Var(name_ref.0.last()), start, end))
                } else {
                    Err(self.error(format!(
                        "qualified variable patterns are not supported: {}",
                        name_ref.0.display(self.session)
                    )))
                }
            }

            Token::UpperIdent(_) => {
                let name_ref = self.parse_name_path()?;
                let end = span_end(&name_ref);
                Ok(sp(SurfacePattern::Con(name_ref, vec![]), start, end))
            }

            Token::LParen => {
                self.advance();

                if self.at(&Token::RParen) {
                    return Err(self.error("unit pattern `()` is not supported yet"));
                }

                let inner = self.parse_pattern()?;
                let end = self.expect(&Token::RParen)?.end;
                Ok(sp(inner.0, start, end))
            }

            Token::LBrack => {
                self.advance();

                if self.at(&Token::RBrack) {
                    let end = self.advance().end;
                    return Ok(list_nil_pattern(self.session, start, end));
                }

                let mut elems = vec![self.parse_pattern()?];

                while self.at(&Token::Comma) {
                    self.advance();
                    elems.push(self.parse_pattern()?);
                }

                let end = self.expect(&Token::RBrack)?.end;
                Ok(list_pattern(self.session, elems, start, end))
            }

            _ => Err(self.error(format!("expected pattern, got {}", self.describe_current()))),
        }
    }

    pub(super) fn is_pattern_arg_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Int(_)
                | Token::Float(_)
                | Token::Char(_)
                | Token::Str(_)
                | Token::Minus
                | Token::Ident(_)
                | Token::UpperIdent(_)
                | Token::LParen
                | Token::LBrack
                | Token::Underscore
        )
    }
}
