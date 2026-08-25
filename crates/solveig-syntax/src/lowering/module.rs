//! Module header and import lowering.
//! Project loading uses these declarations to build the dependency graph.

use super::*;

impl CstToAst<'_> {
    pub(super) fn lower_module_surface(
        &mut self,
    ) -> (ModuleDirectives, Vec<ImportDecl>, SurfaceProgram) {
        let root = self.cst.root();
        let directives = self
            .first_child_of_kind(root, SyntaxKind::ModuleDirectiveList)
            .map_or_else(ModuleDirectives::default, |node| {
                self.lower_module_directives(node)
            });
        let directives = if directives.exports.is_empty()
            && directives.private.is_empty()
            && directives.reexports.is_empty()
        {
            self.lower_direct_module_directives(root)
        } else {
            directives
        };
        let imports = if let Some(node) = self.first_child_of_kind(root, SyntaxKind::ImportList) {
            self.lower_imports(node)
        } else {
            self.lower_direct_imports(root)
        };
        let defs = if let Some(node) = self.first_child_of_kind(root, SyntaxKind::DefinitionList) {
            self.lower_definitions(node)
        } else {
            self.lower_definitions(root)
        };

        (directives, imports, defs)
    }

    fn lower_module_directives(&mut self, node: CstId) -> ModuleDirectives {
        let mut directives = ModuleDirectives::default();
        for child in self.child_nodes(node) {
            let lowered = match self.cst.node(child).kind() {
                SyntaxKind::ExportDecl => self
                    .lower_visibility_directive(child)
                    .map(|decl| directives.exports.push(decl)),
                SyntaxKind::PrivateDecl => self
                    .lower_visibility_directive(child)
                    .map(|decl| directives.private.push(decl)),
                SyntaxKind::ReexportDecl => self
                    .lower_reexport(child)
                    .map(|decl| directives.reexports.push(decl)),
                _ => continue,
            };
            if let Err(error) = lowered
                && !self.node_has_diagnostics(child)
            {
                self.record_error(&error);
            }
        }
        directives
    }

    fn lower_direct_module_directives(&mut self, node: CstId) -> ModuleDirectives {
        let mut directives = ModuleDirectives::default();
        for child in self.child_nodes(node) {
            let lowered = match self.cst.node(child).kind() {
                SyntaxKind::ExportDecl => self
                    .lower_visibility_directive(child)
                    .map(|decl| directives.exports.push(decl)),
                SyntaxKind::PrivateDecl => self
                    .lower_visibility_directive(child)
                    .map(|decl| directives.private.push(decl)),
                SyntaxKind::ReexportDecl => self
                    .lower_reexport(child)
                    .map(|decl| directives.reexports.push(decl)),
                _ => continue,
            };
            if let Err(error) = lowered
                && !self.node_has_diagnostics(child)
            {
                self.record_error(&error);
            }
        }
        directives
    }

    fn lower_visibility_directive(&self, node: CstId) -> Result<VisibilityDecl, ParseError> {
        Ok(VisibilityDecl {
            entities: self.lower_module_entities(node)?,
            span: self.node_span(node),
            cst_id: node,
        })
    }

    fn lower_reexport(&self, node: CstId) -> Result<ReexportDecl, ParseError> {
        let children = self.children(node);
        let mut segments = Vec::new();
        let mut before_selection = true;
        for child in &children {
            let NodeOrToken::Token(token) = child else {
                continue;
            };
            let token = *token;
            match self.cst.token(token).kind {
                Token::LParen => before_selection = false,
                Token::Ident | Token::UpperIdent if before_selection => {
                    segments.push(self.symbol_from_token(token));
                }
                _ => {}
            }
        }
        if segments.is_empty() {
            return Err(self.error_at_node(node, "expected re-exported module name"));
        }

        let selection = children.iter().find_map(|child| match child {
            NodeOrToken::Token(token)
                if self.cst.token(*token).kind == Token::Operator
                    && self.token_text(*token) == "*" =>
            {
                Some(ModuleSelection::All)
            }
            _ => None,
        });
        let selection =
            selection.unwrap_or(ModuleSelection::Only(self.lower_module_entities(node)?));

        Ok(ReexportDecl {
            module: ModuleName(segments),
            selection,
            span: self.node_span(node),
            cst_id: node,
        })
    }

    fn lower_module_entities(&self, node: CstId) -> Result<Vec<ModuleEntity>, ParseError> {
        self.child_nodes_of_kind(node, SyntaxKind::ModuleEntity)
            .into_iter()
            .map(|entity| self.lower_module_entity(entity))
            .collect()
    }

    fn lower_module_entity(&self, node: CstId) -> Result<ModuleEntity, ParseError> {
        let children = self.children(node);
        let mut index = 0;
        let name = self.lower_import_name_from_children(node, &children, &mut index)?;
        let exposure = if children.iter().any(|child| match child {
            NodeOrToken::Token(token) => {
                self.cst.token(*token).kind == Token::ReservedOp && self.token_text(*token) == ".."
            }
            NodeOrToken::Node(_) => false,
        }) {
            EntityExposure::Constructors
        } else {
            EntityExposure::Opaque
        };
        let rename = children.iter().position(|child| {
            matches!(child, NodeOrToken::Token(token) if self.cst.token(*token).kind == Token::As)
        });
        let rename = if let Some(as_index) = rename {
            let mut index = as_index + 1;
            Some(self.lower_import_name_from_children(node, &children, &mut index)?)
        } else {
            None
        };

        Ok(ModuleEntity {
            name,
            exposure,
            rename,
            cst_id: node,
        })
    }

    fn lower_imports(&mut self, node: CstId) -> Vec<ImportDecl> {
        let mut imports = Vec::new();

        for import in self.child_nodes_of_kind(node, SyntaxKind::ImportDecl) {
            match self.lower_import(import) {
                Ok(import) => imports.push(import),
                Err(error) if !self.node_has_diagnostics(import) => self.record_error(&error),
                Err(_) => {}
            }
        }

        imports
    }

    fn lower_direct_imports(&mut self, node: CstId) -> Vec<ImportDecl> {
        let mut imports = Vec::new();

        for import in self.child_nodes_of_kind(node, SyntaxKind::ImportDecl) {
            match self.lower_import(import) {
                Ok(import) => imports.push(import),
                Err(error) if !self.node_has_diagnostics(import) => self.record_error(&error),
                Err(_) => {}
            }
        }

        imports
    }

    fn lower_import(&self, node: CstId) -> Result<ImportDecl, ParseError> {
        let children = self.children(node);
        let mut index = 0usize;
        while let Some(child) = children.get(index) {
            if matches!(child, NodeOrToken::Token(token) if self.cst.token(*token).kind == Token::Import)
            {
                index += 1;
                break;
            }
            index += 1;
        }

        let mut segments = Vec::new();
        loop {
            self.skip_trivia_children(&children, &mut index);
            let Some(child) = children.get(index).copied() else {
                break;
            };
            match child {
                NodeOrToken::Token(token)
                    if matches!(self.cst.token(token).kind, Token::Ident | Token::UpperIdent) =>
                {
                    segments.push(self.symbol_from_token(token));
                    index += 1;
                }
                _ => break,
            }

            self.skip_trivia_children(&children, &mut index);
            if matches!(
                children.get(index),
                Some(NodeOrToken::Token(token)) if self.cst.token(*token).kind == Token::Dot
            ) {
                index += 1;
            } else {
                break;
            }
        }

        if segments.is_empty() {
            return Err(self.error_at_node(node, "expected module name"));
        }

        let module = ModuleName(segments);
        let mut alias = None;

        self.skip_trivia_children(&children, &mut index);
        if matches!(
            children.get(index),
            Some(NodeOrToken::Token(token)) if self.cst.token(*token).kind == Token::As
        ) {
            index += 1;
            self.skip_trivia_children(&children, &mut index);
            let alias_token = self.required_child_token(children.get(index).copied(), node)?;
            if self.cst.token(alias_token).kind != Token::UpperIdent {
                return Err(self.error_at_token(
                    alias_token,
                    format!(
                        "expected constructor, got {}",
                        self.describe_token(alias_token)
                    ),
                ));
            }
            alias = Some((
                self.symbol_from_token(alias_token),
                self.token_span(alias_token),
                node,
            ));
            index += 1;
        }

        self.skip_trivia_children(&children, &mut index);
        let has_selection = matches!(
            children.get(index),
            Some(NodeOrToken::Token(token)) if self.cst.token(*token).kind == Token::LParen
        );
        let imports_all = has_selection
            && children.iter().any(|child| match child {
                NodeOrToken::Token(token) => {
                    self.cst.token(*token).kind == Token::Operator && self.token_text(*token) == "*"
                }
                NodeOrToken::Node(_) => false,
            });
        let selection = if imports_all {
            ModuleSelection::All
        } else {
            ModuleSelection::Only(self.lower_module_entities(node)?)
        };

        Ok(ImportDecl {
            module,
            alias,
            selection,
            span: mk_span(
                self.node_span(node).start as usize,
                self.following_significant_start_after_node(node),
            ),
            cst_id: node,
        })
    }

    fn lower_definitions(&mut self, node: CstId) -> SurfaceProgram {
        let mut defs = Vec::new();

        for child in self.child_nodes(node) {
            if self.cst.node(child).kind() == SyntaxKind::Error {
                continue;
            }
            if !self.node_is_definition(child) {
                continue;
            }

            let leading_docs = self.doc_comment_before_node(child);
            match self.lower_definition(child) {
                Ok(mut def) => {
                    let trailing_docs = self.doc_comment_after_node(child);
                    def.set_docs(self.attached_doc_comment(leading_docs, trailing_docs));
                    let span = self.node_span(child);
                    defs.push(sp(def, span.start as usize, span.end as usize, child));
                }
                Err(error) if !self.node_has_diagnostics(child) => self.record_error(&error),
                Err(_) => {}
            }
        }

        defs
    }

    fn lower_definition(&mut self, node: CstId) -> Result<SurfaceDefinition, ParseError> {
        match self.cst.node(node).kind() {
            SyntaxKind::Signature => self.lower_signature(node),
            SyntaxKind::FunctionDef => self.lower_function_def(node),
            SyntaxKind::FixityDef => self.lower_fixity_def(node),
            SyntaxKind::DataDef => self.lower_data_def(node),
            SyntaxKind::TypeAliasDef => self.lower_type_alias_def(node),
            SyntaxKind::ClassDef => self.lower_class_def(node),
            SyntaxKind::InstanceDef => self.lower_instance_def(node),
            _ => Err(self.error_at_node(node, "expected definition")),
        }
    }

    fn node_is_definition(&self, node: CstId) -> bool {
        matches!(
            self.cst.node(node).kind(),
            SyntaxKind::Signature
                | SyntaxKind::FunctionDef
                | SyntaxKind::FixityDef
                | SyntaxKind::DataDef
                | SyntaxKind::TypeAliasDef
                | SyntaxKind::ClassDef
                | SyntaxKind::InstanceDef
        )
    }
}
