use super::*;

impl Parser<'_> {
    pub(super) fn parse_module(&mut self) -> ParsedModule {
        if self.at(&Token::VOpen) {
            self.advance();
        }

        let imports = self.parse_imports();
        let defs = self.parse_definitions();

        if self.at(&Token::VClose) {
            self.advance();
        }

        ParsedModule {
            file: self.file,
            imports,
            defs,
        }
    }

    pub(super) fn parse_imports(&mut self) -> Vec<ImportDecl> {
        let mut imports = Vec::new();
        while self.at(&Token::Import) {
            match self.parse_import() {
                Ok(import) => imports.push(import),
                Err(error) => {
                    self.report_error(error);
                    self.recover_top_level();
                }
            }
            self.skip_separators();
        }
        imports
    }

    pub(super) fn parse_import(&mut self) -> Result<ImportDecl, ParseError> {
        let start = self.current_start();
        self.expect(&Token::Import)?;

        let mut segments = vec![self.expect_module_name_segment()?];
        while self.at(&Token::Dot) {
            self.advance();
            segments.push(self.expect_module_name_segment()?);
        }

        let module = ModuleName(segments);
        let alias = if self.at(&Token::As) {
            self.advance();
            self.expect_upper()?.0
        } else {
            module.default_alias()
        };

        let spec = if self.at(&Token::LParen) {
            self.advance();
            let mut names = Vec::new();
            while !self.at(&Token::RParen) {
                names.push(self.parse_import_name()?);
                if !self.at(&Token::Comma) {
                    break;
                }
                self.advance();
            }
            self.expect(&Token::RParen)?;
            ImportSpec::Only(names)
        } else {
            ImportSpec::All
        };
        let end = self.current_start();

        Ok(ImportDecl {
            module,
            alias,
            spec,
            span: mk_span(start, end),
        })
    }

    pub(super) fn parse_definitions(&mut self) -> SurfaceProgram {
        let mut defs = Vec::new();

        loop {
            self.skip_top_level_junk();
            if self.at_eof() || self.is_final_module_vclose() {
                break;
            }

            let start = self.current_start();
            match self.parse_definition() {
                Ok(def) => {
                    let end = self.current_start();
                    defs.push(sp(def, start, end));
                }
                Err(error) => {
                    self.report_error(error);
                    self.recover_top_level();
                }
            }
        }

        defs
    }

    pub(super) fn parse_definition(&mut self) -> Result<SurfaceDefinition, ParseError> {
        match self.current_kind() {
            Token::Data => self.parse_data_def(),
            Token::Class => self.parse_class_def(),
            Token::Instance => self.parse_instance_def(),
            Token::Infixl | Token::Infixr | Token::Infix => self.parse_fixity_def(),
            _ => self.parse_func_or_sig(),
        }
    }
}
