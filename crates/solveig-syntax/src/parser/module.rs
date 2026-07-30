//! Module headers, imports, and top-level definition list parsing.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_module_surface(&mut self) {
        self.node_value(SyntaxKind::Module, |parser| {
            if parser.at(&Token::VOpen) {
                parser.advance();
            }

            parser.parse_imports();
            parser.parse_definitions();

            if parser.at(&Token::VClose) {
                parser.advance();
            }

            parser.emit_remaining_tokens();
        });
    }

    pub(super) fn parse_imports(&mut self) {
        if !self.at(&Token::Import) {
            return;
        }

        self.node_value(SyntaxKind::ImportList, |parser| {
            while parser.at(&Token::Import) {
                match parser.parse_import() {
                    Ok(()) => {}
                    Err(error) => parser.recover_top_level_as_error_node(&error),
                }
                parser.skip_separators();
            }
        });
    }

    pub(super) fn parse_import(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::ImportDecl, |parser| {
            parser.expect(&Token::Import)?;

            parser.expect_module_name_segment()?;
            while parser.at(&Token::Dot) {
                parser.advance();
                parser.expect_module_name_segment()?;
            }

            if parser.at(&Token::As) {
                parser.advance();
                parser.expect_upper_span()?;
            }

            if parser.at(&Token::LParen) {
                parser.advance();
                while !parser.at(&Token::RParen) {
                    parser.parse_import_name()?;
                    if !parser.at(&Token::Comma) {
                        break;
                    }
                    parser.advance();
                }
                parser.expect(&Token::RParen)?;
            }

            Ok(())
        })
    }

    pub(super) fn parse_definitions(&mut self) {
        self.node_value(SyntaxKind::DefinitionList, |parser| {
            loop {
                parser.skip_top_level_junk();
                if parser.at_eof() || parser.is_final_module_vclose() {
                    break;
                }

                match parser.parse_definition() {
                    Ok(()) => {}
                    Err(error) => parser.recover_top_level_as_error_node(&error),
                }
            }
        });
    }

    pub(super) fn parse_definition(&mut self) -> Result<(), ParseError> {
        match self.current_kind() {
            Token::Data => self.parse_data_def(),
            Token::Type => self.parse_type_alias_def(),
            Token::Class => self.parse_class_def(),
            Token::Instance => self.parse_instance_def(),
            Token::Infixl | Token::Infixr | Token::Infix => self.parse_fixity_def(),
            _ => self.parse_func_or_sig(),
        }
    }
}
