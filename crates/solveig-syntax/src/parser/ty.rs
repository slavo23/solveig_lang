//! Type syntax parsing.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_predicate(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::Predicate, |parser| {
            let class_name = parser.parse_name_path()?;
            if !is_ctor_like(&class_name.last_segment) {
                return Err(parser.error(format!("expected class name, got {}", class_name.text)));
            }

            let mut argument_count = 0usize;
            while parser.is_type_atom_start() {
                parser.parse_type_atom()?;
                argument_count += 1;
            }

            if argument_count == 0 {
                return Err(parser.error("expected at least one type argument in predicate"));
            }

            Ok(())
        })
    }

    pub(super) fn parse_predicate_list(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::PredicateList, |parser| {
            if parser.at(&Token::LParen) {
                parser.advance();
                parser.parse_predicate()?;
                while parser.at(&Token::Comma) {
                    parser.advance();
                    parser.parse_predicate()?;
                }
                parser.expect(&Token::RParen)?;
            } else {
                parser.parse_predicate()?;
            }
            Ok(())
        })
    }

    pub(super) fn parse_type(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::Type, |parser| {
            if parser.at(&Token::Forall) {
                parser.advance();
                while matches!(parser.current_kind(), Token::Ident) {
                    parser.expect_ident_span()?;
                }
                parser.expect(&Token::Dot)?;
                return parser.parse_type();
            }

            parser.parse_qualified_type()
        })
    }

    pub(super) fn parse_qualified_type(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::QualifiedType, |parser| {
            let checkpoint = parser.checkpoint();

            if parser.parse_predicate_list().is_ok() && parser.at(&Token::FatArrow) {
                parser.advance();
                return parser.parse_type_arrow();
            }

            parser.restore(checkpoint);
            parser.parse_type_arrow()
        })
    }

    pub(super) fn parse_type_arrow(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::TypeArrow, |parser| {
            parser.parse_type_app()?;
            if parser.at(&Token::Arrow) {
                parser.advance();
                parser.parse_type()?;
            }
            Ok(())
        })
    }

    pub(super) fn parse_type_app(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::TypeApp, |parser| {
            parser.parse_type_atom()?;
            while parser.is_type_atom_start() {
                parser.parse_type_atom()?;
            }
            Ok(())
        })
    }

    pub(super) fn is_type_atom_start(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Ident | Token::UpperIdent | Token::LParen | Token::LBrack
        )
    }

    pub(super) fn parse_type_atom(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::TypeAtom, |parser| match parser.current_kind() {
            Token::Ident | Token::UpperIdent => {
                let name_path = parser.parse_name_path()?;
                if !is_ctor_like(&name_path.last_segment) && name_path.segment_count > 1 {
                    return Err(parser.error(format!(
                        "qualified type variable is not allowed: {}",
                        name_path.text
                    )));
                }
                Ok(())
            }
            Token::LParen => {
                parser.advance();
                if parser.at(&Token::RParen) {
                    parser.advance();
                    return Ok(());
                }
                parser.parse_type()?;
                parser.expect(&Token::RParen)?;
                Ok(())
            }
            Token::LBrack => {
                parser.advance();
                parser.parse_type()?;
                parser.expect(&Token::RBrack)?;
                Ok(())
            }
            _ => Err(parser.error(format!("expected type, got {}", parser.describe_current()))),
        })
    }
}
