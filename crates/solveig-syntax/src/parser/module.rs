//! Module headers, imports, and top-level definition list parsing.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_module_surface(&mut self) {
        self.node_value(SyntaxKind::Module, |parser| {
            if parser.at(&Token::VOpen) {
                parser.advance();
            }

            parser.parse_module_directives();
            parser.parse_imports();
            parser.parse_definitions();

            if parser.at(&Token::VClose) {
                parser.advance();
            }

            parser.emit_remaining_tokens();
        });
    }

    fn parse_module_directives(&mut self) {
        if !matches!(
            self.current_kind(),
            Token::Export | Token::Private | Token::Reexport
        ) {
            return;
        }

        self.node_value(SyntaxKind::ModuleDirectiveList, |parser| {
            while matches!(
                parser.current_kind(),
                Token::Export | Token::Private | Token::Reexport
            ) {
                match parser.parse_module_directive() {
                    Ok(()) => {}
                    Err(error) => parser.recover_top_level_as_error_node(&error),
                }
                parser.skip_separators();
            }
        });
    }

    fn parse_module_directive(&mut self) -> Result<(), ParseError> {
        match self.current_kind() {
            Token::Export => self.parse_visibility_directive(Token::Export, SyntaxKind::ExportDecl),
            Token::Private => {
                self.parse_visibility_directive(Token::Private, SyntaxKind::PrivateDecl)
            }
            Token::Reexport => self.parse_reexport(),
            _ => Err(self.error(format!(
                "expected module directive, got {}",
                self.describe_current()
            ))),
        }
    }

    fn parse_visibility_directive(
        &mut self,
        keyword: Token,
        kind: SyntaxKind,
    ) -> Result<(), ParseError> {
        self.node(kind, |parser| {
            parser.expect(&keyword)?;
            parser.parse_module_selection(false, false)
        })
    }

    fn parse_reexport(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::ReexportDecl, |parser| {
            parser.expect(&Token::Reexport)?;
            parser.expect_module_name_segment()?;
            while parser.at(&Token::Dot) {
                parser.advance();
                parser.expect_module_name_segment()?;
            }
            parser.parse_module_selection(true, true)
        })
    }

    fn parse_module_selection(
        &mut self,
        allow_rename: bool,
        allow_all: bool,
    ) -> Result<(), ParseError> {
        self.expect(&Token::LParen)?;
        if self.at(&Token::Operator) && self.current_source_text() == "*" {
            if !allow_all {
                return Err(self.error("`*` is only allowed in re-exports and imports"));
            }
            self.advance();
        } else {
            while !self.at(&Token::RParen) {
                self.parse_module_entity(allow_rename)?;
                if !self.at(&Token::Comma) {
                    break;
                }
                self.advance();
            }
        }
        self.expect(&Token::RParen).map(|_| ())
    }

    fn parse_module_entity(&mut self, allow_rename: bool) -> Result<(), ParseError> {
        self.node(SyntaxKind::ModuleEntity, |parser| {
            let data_name = parser.at(&Token::UpperIdent);
            parser.parse_import_name()?;
            if data_name && parser.at(&Token::LParen) {
                parser.advance();
                if !parser.at(&Token::ReservedOp) || parser.current_source_text() != ".." {
                    return Err(parser.error("expected `..` to expose all constructors"));
                }
                parser.advance();
                parser.expect(&Token::RParen)?;
            }
            if parser.at(&Token::As) {
                if !allow_rename {
                    return Err(parser.error("renames are only allowed in re-exports and imports"));
                }
                parser.advance();
                parser.parse_import_name()?;
            }
            Ok(())
        })
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
                parser.parse_module_selection(true, true)?;
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
