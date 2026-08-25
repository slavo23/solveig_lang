//! Pattern syntax parsing.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_pattern(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::Pattern, |parser| {
            parser.parse_pattern_head()?;
            if parser.starts_backticked_constructor_name() {
                parser.parse_backticked_constructor_name()?;
                parser.parse_pattern()?;
            } else if parser.at(&Token::Cons) {
                parser.advance();
                parser.parse_pattern()?;
            }
            Ok(())
        })
    }

    pub(super) fn starts_backticked_constructor_name(&self) -> bool {
        self.at(&Token::Backtick)
            && matches!(self.peek_kind(1), Some(Token::UpperIdent))
            && matches!(self.peek_kind(2), Some(Token::Backtick))
    }

    pub(super) fn parse_backticked_constructor_name(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::BacktickOperator, |parser| {
            parser.expect(&Token::Backtick)?;
            parser.expect_upper_span()?;
            parser.expect(&Token::Backtick)?;
            Ok(())
        })
    }

    pub(super) fn parse_pattern_head(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::PatternHead, |parser| {
            if parser.tuple_constructor_arity().is_some() {
                parser.parse_tuple_constructor()?;
                while parser.is_pattern_arg_start() {
                    parser.parse_pattern_arg()?;
                }
                Ok(())
            } else if parser.current_name_path_is_constructor() {
                parser.parse_name_path()?;
                while parser.is_pattern_arg_start() {
                    parser.parse_pattern_arg()?;
                }
                Ok(())
            } else {
                parser.parse_pattern_arg()
            }
        })
    }

    pub(super) fn parse_pattern_arg(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::PatternArg, |parser| {
            match parser.current_kind() {
                Token::Int | Token::Float | Token::Char | Token::Str | Token::Underscore => {
                    parser.advance();
                    Ok(())
                }
                Token::Minus => {
                    parser.advance();
                    match parser.current_kind() {
                        Token::Int | Token::Float => {
                            parser.advance();
                            Ok(())
                        }
                        _ => Err(parser.error(format!(
                            "expected numeric literal after `-` in pattern, got {}",
                            parser.describe_current()
                        ))),
                    }
                }
                Token::Ident => {
                    let name_path = parser.parse_name_path()?;
                    if name_path.segment_count == 1 || is_ctor_like(&name_path.last_segment) {
                        Ok(())
                    } else {
                        Err(parser.error(format!(
                            "qualified variable patterns are not supported: {}",
                            name_path.text
                        )))
                    }
                }
                Token::UpperIdent => {
                    parser.parse_name_path()?;
                    Ok(())
                }
                Token::LParen => {
                    if parser.tuple_constructor_arity().is_some() {
                        return parser.parse_tuple_constructor();
                    }
                    let kind = if parser.parenthesized_has_top_level_comma() {
                        SyntaxKind::TuplePattern
                    } else {
                        SyntaxKind::PatternArg
                    };
                    if kind == SyntaxKind::TuplePattern {
                        return parser.node(kind, |parser| {
                            parser.advance();
                            parser.parse_pattern()?;
                            let mut arity = 1usize;
                            while parser.at(&Token::Comma) {
                                parser.advance();
                                arity += 1;
                                parser.check_tuple_arity(arity)?;
                                if parser.at(&Token::RParen) {
                                    return Err(
                                        parser.error("tuple patterns may not omit elements")
                                    );
                                }
                                parser.parse_pattern()?;
                            }
                            parser.expect(&Token::RParen)?;
                            Ok(())
                        });
                    }

                    parser.advance();

                    if parser.at(&Token::RParen) {
                        return Err(parser.error("unit pattern `()` is not supported yet"));
                    }

                    parser.parse_pattern()?;
                    parser.expect(&Token::RParen)?;
                    Ok(())
                }
                Token::LBrack => {
                    parser.advance();

                    if parser.at(&Token::RBrack) {
                        parser.advance();
                        return Ok(());
                    }

                    parser.parse_pattern()?;
                    while parser.at(&Token::Comma) {
                        parser.advance();
                        parser.parse_pattern()?;
                    }

                    parser.expect(&Token::RBrack)?;
                    Ok(())
                }
                _ => Err(parser.error(format!(
                    "expected pattern, got {}",
                    parser.describe_current()
                ))),
            }
        })
    }

    pub(super) fn is_pattern_arg_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Int
                | Token::Float
                | Token::Char
                | Token::Str
                | Token::Minus
                | Token::Ident
                | Token::UpperIdent
                | Token::LParen
                | Token::LBrack
                | Token::Underscore
        )
    }
}
