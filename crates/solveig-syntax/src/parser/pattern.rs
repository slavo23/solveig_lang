//! Pattern parser for function equations, case arms, and bindings.
//! Later checker passes lower these patterns into coverage matrices.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_pattern(&mut self) -> Result<S<SurfacePattern>, ParseError> {
        self.node(SyntaxKind::Pattern, |this| {
            let head = this.parse_pattern_head()?;
            if this.at(&Token::Cons) {
                this.advance();
                let tail = this.parse_pattern()?;
                Ok(cons_pattern(this.session, head, tail))
            } else {
                Ok(head)
            }
        })
    }

    pub(super) fn parse_pattern_head(&mut self) -> Result<S<SurfacePattern>, ParseError> {
        self.node(SyntaxKind::PatternHead, |this| {
            if matches!(this.current_kind(), Token::UpperIdent(_)) {
                let start = this.current_start();
                let name_ref = this.parse_name_path()?;
                let mut args = Vec::new();
                while this.is_pattern_arg_start() {
                    args.push(this.parse_pattern_arg()?);
                }
                let end = args.last().map_or_else(|| span_end(&name_ref), span_end);
                Ok(sp(SurfacePattern::Con(name_ref, args), start, end))
            } else {
                this.parse_pattern_arg()
            }
        })
    }

    pub(super) fn parse_pattern_arg(&mut self) -> Result<S<SurfacePattern>, ParseError> {
        self.node(SyntaxKind::PatternArg, |this| {
            let start = this.current_start();

            match this.current_kind() {
                Token::Int(value) => {
                    let value = *value;
                    let token = this.advance();
                    Ok(int_pattern(value, token.start, token.end))
                }

                Token::Float(value) => {
                    let value = *value;
                    let token = this.advance();
                    Ok(literal_pattern(
                        SurfaceLiteral::Float(value),
                        token.start,
                        token.end,
                    ))
                }

                Token::Char(value) => {
                    let value = *value;
                    let token = this.advance();
                    Ok(literal_pattern(
                        SurfaceLiteral::Char(value),
                        token.start,
                        token.end,
                    ))
                }

                Token::Str(value) => {
                    let value = value.clone();
                    let token = this.advance();
                    Ok(literal_pattern(
                        SurfaceLiteral::Str(value),
                        token.start,
                        token.end,
                    ))
                }

                Token::Minus => {
                    let minus = this.advance();
                    match this.current_kind() {
                        Token::Int(value) => {
                            let value = value.checked_neg().ok_or_else(|| {
                                this.error("integer literal pattern is too small")
                            })?;

                            let int_token = this.advance();
                            Ok(int_pattern(value, minus.start, int_token.end))
                        }

                        Token::Float(value) => {
                            let value = -*value;
                            let float_token = this.advance();
                            Ok(literal_pattern(
                                SurfaceLiteral::Float(value),
                                minus.start,
                                float_token.end,
                            ))
                        }

                        _ => Err(this.error(format!(
                            "expected numeric literal after `-` in pattern, got {}",
                            this.describe_current()
                        ))),
                    }
                }

                Token::Underscore => {
                    let token = this.advance();
                    Ok(sp(SurfacePattern::Wild, token.start, token.end))
                }

                Token::Ident(_) => {
                    let name_ref = this.parse_name_path()?;
                    let end = span_end(&name_ref);

                    if name_ref.0.segments.len() == 1 {
                        Ok(sp(SurfacePattern::Var(name_ref.0.last()), start, end))
                    } else {
                        Err(this.error(format!(
                            "qualified variable patterns are not supported: {}",
                            name_ref.0.display(this.session)
                        )))
                    }
                }

                Token::UpperIdent(_) => {
                    let name_ref = this.parse_name_path()?;
                    let end = span_end(&name_ref);
                    Ok(sp(SurfacePattern::Con(name_ref, vec![]), start, end))
                }

                Token::LParen => {
                    this.advance();

                    if this.at(&Token::RParen) {
                        return Err(this.error("unit pattern `()` is not supported yet"));
                    }

                    let inner = this.parse_pattern()?;
                    let end = this.expect(&Token::RParen)?.end;
                    Ok(sp(inner.0, start, end))
                }

                Token::LBrack => {
                    this.advance();

                    if this.at(&Token::RBrack) {
                        let end = this.advance().end;
                        return Ok(list_nil_pattern(this.session, start, end));
                    }

                    let mut elems = vec![this.parse_pattern()?];

                    while this.at(&Token::Comma) {
                        this.advance();
                        elems.push(this.parse_pattern()?);
                    }

                    let end = this.expect(&Token::RBrack)?.end;
                    Ok(list_pattern(this.session, elems, start, end))
                }

                _ => Err(this.error(format!("expected pattern, got {}", this.describe_current()))),
            }
        })
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
