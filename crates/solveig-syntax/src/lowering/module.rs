//! Module header and import lowering.
//! Project loading uses these declarations to build the dependency graph.

use super::*;

impl CstToAst<'_> {
    pub(super) fn lower_module_surface(&mut self) -> (Vec<ImportDecl>, SurfaceProgram) {
        let root = self.cst.root();
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

        (imports, defs)
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
        let mut alias = module.default_alias();

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
            alias = self.symbol_from_token(alias_token);
            index += 1;
        }

        let mut names = Vec::new();
        let mut selected_names = Vec::new();

        self.skip_trivia_children(&children, &mut index);
        if matches!(
            children.get(index),
            Some(NodeOrToken::Token(token)) if self.cst.token(*token).kind == Token::LParen
        ) {
            index += 1;
            loop {
                self.skip_trivia_children(&children, &mut index);
                if matches!(
                    children.get(index),
                    Some(NodeOrToken::Token(token)) if self.cst.token(*token).kind == Token::RParen
                ) {
                    break;
                }

                let selected = self.lower_import_name_from_children(node, &children, &mut index)?;
                names.push(selected.0);
                selected_names.push(selected);

                self.skip_trivia_children(&children, &mut index);
                if matches!(
                    children.get(index),
                    Some(NodeOrToken::Token(token)) if self.cst.token(*token).kind == Token::Comma
                ) {
                    index += 1;
                } else {
                    break;
                }
            }
        }

        let spec = if names.is_empty() {
            ImportSpec::All
        } else {
            ImportSpec::Only(names)
        };

        Ok(ImportDecl {
            module,
            alias,
            spec,
            selected_names,
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
