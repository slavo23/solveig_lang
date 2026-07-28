//! Module header and import parsing.
//! Project loading uses these declarations to build the dependency graph.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_module_surface(&mut self) -> (Vec<ImportDecl>, SurfaceProgram) {
        self.node_value(SyntaxKind::Module, |this| {
            if this.at(&Token::VOpen) {
                this.advance();
            }

            let imports = this.parse_imports();
            let defs = this.parse_definitions();

            if this.at(&Token::VClose) {
                this.advance();
            }

            this.emit_remaining_tokens();
            (imports, defs)
        })
    }

    pub(super) fn parse_imports(&mut self) -> Vec<ImportDecl> {
        self.node_value(SyntaxKind::ImportList, |this| {
            let mut imports = Vec::new();
            while this.at(&Token::Import) {
                match this.parse_import() {
                    Ok(import) => imports.push(import),
                    Err(error) => {
                        this.recover_top_level_as_error_node(&error);
                    }
                }
                this.skip_separators();
            }
            imports
        })
    }

    pub(super) fn parse_import(&mut self) -> Result<ImportDecl, ParseError> {
        self.node(SyntaxKind::ImportDecl, |this| {
            let start = this.current_start();
            this.expect(&Token::Import)?;

            let mut segments = vec![this.expect_module_name_segment()?];
            while this.at(&Token::Dot) {
                this.advance();
                segments.push(this.expect_module_name_segment()?);
            }

            let module = ModuleName(segments);
            let alias = if this.at(&Token::As) {
                this.advance();
                this.expect_upper()?.0
            } else {
                module.default_alias()
            };

            let spec = if this.at(&Token::LParen) {
                this.advance();
                let mut names = Vec::new();
                while !this.at(&Token::RParen) {
                    names.push(this.parse_import_name()?);
                    if !this.at(&Token::Comma) {
                        break;
                    }
                    this.advance();
                }
                this.expect(&Token::RParen)?;
                ImportSpec::Only(names)
            } else {
                ImportSpec::All
            };
            let end = this.current_start();

            Ok(ImportDecl {
                module,
                alias,
                spec,
                span: mk_span(start, end),
                cst_id: this.current_cst_id(),
            })
        })
    }

    pub(super) fn parse_definitions(&mut self) -> SurfaceProgram {
        self.node_value(SyntaxKind::DefinitionList, |this| {
            let mut defs = Vec::new();

            loop {
                this.skip_top_level_junk();
                if this.at_eof() || this.is_final_module_vclose() {
                    break;
                }

                let start = this.current_start();
                match this.parse_definition() {
                    Ok(def) => {
                        let end = this.current_start();
                        defs.push(sp(def, start, end, this.completed_cst_id()));
                    }
                    Err(error) => {
                        this.recover_top_level_as_error_node(&error);
                    }
                }
            }

            defs
        })
    }

    pub(super) fn parse_definition(&mut self) -> Result<SurfaceDefinition, ParseError> {
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
