//! Top-level declaration parsing.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_func_or_sig(&mut self) -> Result<(), ParseError> {
        let kind = self.current_value_definition_kind();
        self.node(kind, |parser| {
            parser.expect_value_name()?;

            if parser.at(&Token::Colon) {
                parser.advance();
                parser.parse_type()?;
                return Ok(());
            }

            while parser.is_pattern_arg_start() {
                parser.parse_pattern_arg()?;
            }

            parser.expect(&Token::Eq)?;
            parser.parse_expr()
        })
    }

    pub(super) fn current_value_definition_kind(&self) -> SyntaxKind {
        let token_after_name = if self.starts_parenthesized_operator_name() {
            3
        } else if self.at(&Token::Ident) {
            1
        } else {
            return SyntaxKind::FunctionDef;
        };

        if matches!(self.peek_kind(token_after_name), Some(Token::Colon)) {
            SyntaxKind::Signature
        } else {
            SyntaxKind::FunctionDef
        }
    }

    pub(super) fn parse_fixity_def(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::FixityDef, |parser| {
            match parser.current_kind() {
                Token::Infixl | Token::Infixr | Token::Infix => {
                    parser.advance();
                }
                _ => {
                    return Err(parser.error(format!(
                        "expected fixity declaration, got {}",
                        parser.describe_current()
                    )));
                }
            }

            if matches!(parser.current_kind(), Token::Int) {
                parser.advance();
            }

            loop {
                parser.parse_fixity_operator_name()?;
                if !parser.at(&Token::Comma) {
                    break;
                }
                parser.advance();
            }

            Ok(())
        })
    }

    pub(super) fn parse_fixity_operator_name(&mut self) -> Result<(), ParseError> {
        if self.starts_backticked_operator_name() {
            self.parse_backticked_operator_name()
        } else if self.starts_parenthesized_operator_name() {
            self.parse_parenthesized_operator_name()
        } else {
            self.expect_operator_span(false)
        }
    }

    pub(super) fn parse_data_def(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::DataDef, |parser| {
            parser.expect(&Token::Data)?;
            parser.expect_upper_span()?;

            while matches!(parser.current_kind(), Token::Ident) {
                parser.expect_ident_span()?;
            }

            if !parser.at(&Token::Eq) {
                return Ok(());
            }
            parser.advance();

            loop {
                if parser.at(&Token::Pipe) {
                    parser.advance();
                }

                parser.parse_constructor()?;

                if !parser.at(&Token::Pipe) {
                    break;
                }
            }

            Ok(())
        })
    }

    pub(super) fn parse_type_alias_def(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::TypeAliasDef, |parser| {
            parser.expect(&Token::Type)?;
            parser.expect_upper_span()?;

            while matches!(parser.current_kind(), Token::Ident) {
                parser.expect_ident_span()?;
            }

            parser.expect(&Token::Eq)?;
            parser.parse_type()
        })
    }

    pub(super) fn parse_constructor(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::Constructor, |parser| {
            parser.expect_upper_span()?;
            while parser.is_type_atom_start() {
                parser.parse_type_atom()?;
            }
            Ok(())
        })
    }

    pub(super) fn parse_optional_context(&mut self) {
        let checkpoint = self.checkpoint();
        if self.parse_predicate_list().is_ok() && self.at(&Token::FatArrow) {
            self.advance();
        } else {
            self.restore(checkpoint);
        }
    }

    pub(super) fn parse_class_def(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::ClassDef, |parser| {
            parser.expect(&Token::Class)?;
            parser.parse_optional_context();

            parser.expect_upper_span()?;

            let mut type_parameter_count = 0usize;
            while matches!(parser.current_kind(), Token::Ident) {
                parser.expect_ident_span()?;
                type_parameter_count += 1;
            }

            if type_parameter_count == 0 {
                return Err(
                    parser.error("class declaration needs at least one type parameter".to_string())
                );
            }

            parser.expect(&Token::Where)?;

            if parser.at(&Token::VOpen) {
                parser.advance();
            }

            loop {
                parser.skip_separators();
                if parser.at(&Token::VClose) || parser.at_eof() {
                    break;
                }

                if parser.at(&Token::Default) {
                    parser.advance();
                    parser.expect_value_name()?;
                    while matches!(parser.current_kind(), Token::Ident | Token::Underscore) {
                        parser.expect_binder_span()?;
                    }
                    parser.expect(&Token::Eq)?;
                    parser.parse_expr()?;
                    continue;
                }

                parser.expect_value_name()?;
                parser.expect(&Token::Colon)?;
                parser.parse_type()?;
            }

            if parser.at(&Token::VClose) {
                parser.advance();
            }

            Ok(())
        })
    }

    pub(super) fn parse_instance_def(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::InstanceDef, |parser| {
            parser.expect(&Token::Instance)?;
            parser.parse_optional_context();

            parser.expect_upper_span()?;

            let mut argument_count = 0usize;
            while parser.is_type_atom_start() {
                parser.parse_type_atom()?;
                argument_count += 1;
            }

            if argument_count == 0 {
                return Err(parser.error("instance head needs at least one type argument"));
            }

            parser.expect(&Token::Where)?;

            if parser.at(&Token::VOpen) {
                parser.advance();
            }

            loop {
                parser.skip_separators();
                if parser.at(&Token::VClose) || parser.at_eof() {
                    break;
                }
                parser.expect_value_name()?;
                while matches!(parser.current_kind(), Token::Ident | Token::Underscore) {
                    parser.expect_binder_span()?;
                }
                parser.expect(&Token::Eq)?;
                parser.parse_expr()?;
            }

            if parser.at(&Token::VClose) {
                parser.advance();
            }

            Ok(())
        })
    }
}
