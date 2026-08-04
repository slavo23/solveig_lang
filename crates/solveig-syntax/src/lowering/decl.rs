//! Declaration lowering for modules, data types, classes, instances, and fixities.
//! Lowered declarations feed fixity resolution and project API construction.

use super::*;
use solveig_base::ice;

impl CstToAst<'_> {
    pub(super) fn lower_signature(&mut self, node: CstId) -> Result<SurfaceDefinition, ParseError> {
        let children = self.children(node);
        let mut index = 0usize;
        let name = self.lower_value_name_from_children(node, &children, &mut index)?;
        let ty =
            self.required_child(node, self.last_type_child(node), "expected signature type")?;

        Ok(SurfaceDefinition::SigDef {
            name,
            ty: self.lower_type(ty)?,
            docs: None,
        })
    }

    pub(super) fn lower_function_def(
        &mut self,
        node: CstId,
    ) -> Result<SurfaceDefinition, ParseError> {
        let children = self.children(node);
        let mut index = 0usize;
        let name = self.lower_value_name_from_children(node, &children, &mut index)?;
        let mut params = Vec::new();

        while index < children.len() {
            self.skip_trivia_children(&children, &mut index);
            let Some(child) = children.get(index).copied() else {
                break;
            };
            match child {
                NodeOrToken::Token(token) if self.cst.token(token).kind == Token::Eq => {
                    index += 1;
                    break;
                }
                NodeOrToken::Token(token)
                    if matches!(self.cst.token(token).kind, Token::Ident | Token::Underscore) =>
                {
                    params.push(self.binder_span_from_token(token, node));
                    index += 1;
                }
                _ => index += 1,
            }
        }

        let body = self.required_child(
            node,
            children.iter().skip(index).find_map(|child| match child {
                NodeOrToken::Node(expr) if self.node_is_expr_like(*expr) => Some(*expr),
                _ => None,
            }),
            "expected function body",
        )?;

        Ok(SurfaceDefinition::FunDef {
            name,
            body: self.lower_expr(body)?,
            params,
            docs: None,
        })
    }

    pub(super) fn lower_fixity_def(
        &mut self,
        node: CstId,
    ) -> Result<SurfaceDefinition, ParseError> {
        let children = self.children(node);
        let mut index = 0usize;
        let assoc_token = self.required_token(
            node,
            self.direct_meaningful_tokens(node)
                .into_iter()
                .find(|token| {
                    matches!(
                        self.cst.token(*token).kind,
                        Token::Infixl | Token::Infixr | Token::Infix
                    )
                }),
            "expected fixity declaration",
        )?;
        let assoc = match self.cst.token(assoc_token).kind {
            Token::Infixl => Assoc::Left,
            Token::Infixr => Assoc::Right,
            Token::Infix => Assoc::None,
            _ => ice!("fixity token was filtered above"),
        };

        while let Some(child) = children.get(index) {
            if matches!(child, NodeOrToken::Token(token) if *token == assoc_token) {
                index += 1;
                break;
            }
            index += 1;
        }

        self.skip_trivia_children(&children, &mut index);
        let prec = if let Some(NodeOrToken::Token(token)) = children.get(index).copied() {
            if self.cst.token(token).kind == Token::Int {
                index += 1;
                let value = self.lower_int_literal(token)?;
                u8::try_from(value)
                    .ok()
                    .filter(|value| *value <= 9)
                    .ok_or_else(|| {
                        self.error_at_token(token, "fixity precedence must be in the range 0..9")
                    })?
            } else {
                Fixity::DEFAULT.prec
            }
        } else {
            Fixity::DEFAULT.prec
        };

        let mut ops = Vec::new();
        while index < children.len() {
            self.skip_trivia_children(&children, &mut index);
            if matches!(
                children.get(index),
                Some(NodeOrToken::Token(token)) if self.cst.token(*token).kind == Token::Comma
            ) {
                index += 1;
                continue;
            }

            let Some(child) = children.get(index).copied() else {
                break;
            };
            match child {
                NodeOrToken::Node(child)
                    if self.cst.node(child).kind() == SyntaxKind::BacktickOperator =>
                {
                    let (op, start, end, cst_id) =
                        self.lower_backticked_operator_name(child, false)?;
                    ops.push(sp(op, start, end, cst_id));
                    index += 1;
                }
                NodeOrToken::Token(token) if self.cst.token(token).kind == Token::LParen => {
                    ops.push(self.lower_value_name_from_children(node, &children, &mut index)?);
                }
                NodeOrToken::Token(token)
                    if self.child_is_operator(NodeOrToken::Token(token), false) =>
                {
                    let symbol = self.operator_symbol_from_token(token, false)?;
                    let source_token = self.cst.token(token);
                    ops.push(sp(symbol, source_token.start(), source_token.end(), node));
                    index += 1;
                }
                _ => break,
            }
        }

        Ok(SurfaceDefinition::FixityDef {
            fixity: Fixity { prec, assoc },
            ops,
            docs: None,
        })
    }

    pub(super) fn lower_data_def(&mut self, node: CstId) -> Result<SurfaceDefinition, ParseError> {
        let children = self.children(node);
        let data_index = self.index_after_token(&children, Token::Data).unwrap_or(0);
        let name = self.required_token(
            node,
            children
                .iter()
                .skip(data_index)
                .find_map(|child| match child {
                    NodeOrToken::Token(token)
                        if self.cst.token(*token).kind == Token::UpperIdent =>
                    {
                        Some(*token)
                    }
                    _ => None,
                }),
            "expected data type name",
        )?;
        let mut params = Vec::new();
        let mut seen_name = false;

        for child in &children {
            match child {
                NodeOrToken::Token(token) if *token == name => seen_name = true,
                NodeOrToken::Token(token)
                    if seen_name && self.cst.token(*token).kind == Token::Eq =>
                {
                    break;
                }
                NodeOrToken::Token(token)
                    if seen_name && self.cst.token(*token).kind == Token::Ident =>
                {
                    params.push(self.symbol_span_from_token(*token, node));
                }
                _ => {}
            }
        }

        let constructors = self
            .child_nodes_of_kind(node, SyntaxKind::Constructor)
            .into_iter()
            .map(|ctor| {
                let span = self.node_span(ctor);
                self.lower_constructor(ctor)
                    .map(|ctor_value| sp(ctor_value, span.start as usize, span.end as usize, ctor))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(SurfaceDefinition::DataDef {
            name: self.symbol_span_from_token(name, node),
            ty_params: params,
            constructors,
            docs: None,
        })
    }

    pub(super) fn lower_type_alias_def(
        &mut self,
        node: CstId,
    ) -> Result<SurfaceDefinition, ParseError> {
        let children = self.children(node);
        let type_index = self.index_after_token(&children, Token::Type).unwrap_or(0);
        let name = self.required_token(
            node,
            children
                .iter()
                .skip(type_index)
                .find_map(|child| match child {
                    NodeOrToken::Token(token)
                        if self.cst.token(*token).kind == Token::UpperIdent =>
                    {
                        Some(*token)
                    }
                    _ => None,
                }),
            "expected type alias name",
        )?;
        let mut params = Vec::new();
        let mut seen_name = false;

        for child in &children {
            match child {
                NodeOrToken::Token(token) if *token == name => seen_name = true,
                NodeOrToken::Token(token)
                    if seen_name && self.cst.token(*token).kind == Token::Eq =>
                {
                    break;
                }
                NodeOrToken::Token(token)
                    if seen_name && self.cst.token(*token).kind == Token::Ident =>
                {
                    params.push(self.symbol_span_from_token(*token, node));
                }
                _ => {}
            }
        }

        let ty = self.required_child(node, self.last_type_child(node), "expected alias body")?;
        Ok(SurfaceDefinition::TypeAliasDef {
            name: self.symbol_span_from_token(name, node),
            ty_params: params,
            ty: self.lower_type(ty)?,
            docs: None,
        })
    }

    pub(super) fn lower_constructor(
        &mut self,
        node: CstId,
    ) -> Result<SurfaceConstructor, ParseError> {
        let leading_docs = self.doc_comment_before_node(node);
        let name = self.required_token(
            node,
            self.direct_meaningful_tokens(node)
                .into_iter()
                .find(|token| self.cst.token(*token).kind == Token::UpperIdent),
            "expected constructor name",
        )?;
        let param_nodes = self
            .child_nodes(node)
            .into_iter()
            .filter(|child| self.node_is_type_like(*child))
            .collect::<Vec<_>>();
        let mut params = Vec::new();
        for param in param_nodes {
            params.push(self.lower_type_atom(param)?);
        }
        let trailing_docs = self.doc_comment_after_node(node);

        Ok(SurfaceConstructor {
            name: self.symbol_span_from_token(name, node),
            params,
            docs: self.attached_doc_comment(leading_docs, trailing_docs),
        })
    }

    pub(super) fn lower_class_def(&mut self, node: CstId) -> Result<SurfaceDefinition, ParseError> {
        let children = self.children(node);
        let superclasses = if self.node_has_token(node, Token::FatArrow) {
            self.lower_context_predicates(node)?
        } else {
            Vec::new()
        };
        let class_index = self.index_after_token(&children, Token::Class).unwrap_or(0);
        let name = self.required_token(
            node,
            children
                .iter()
                .skip(class_index)
                .find_map(|child| match child {
                    NodeOrToken::Token(token)
                        if self.cst.token(*token).kind == Token::UpperIdent =>
                    {
                        Some(*token)
                    }
                    _ => None,
                }),
            "expected class name",
        )?;
        let mut ty_params = Vec::new();
        let mut seen_name = false;

        for child in &children {
            match child {
                NodeOrToken::Token(token) if *token == name => seen_name = true,
                NodeOrToken::Token(token)
                    if seen_name && self.cst.token(*token).kind == Token::Where =>
                {
                    break;
                }
                NodeOrToken::Token(token)
                    if seen_name && self.cst.token(*token).kind == Token::Ident =>
                {
                    ty_params.push(self.symbol_span_from_token(*token, node));
                }
                _ => {}
            }
        }

        if ty_params.is_empty() {
            return Err(
                self.error_at_node(node, "class declaration needs at least one type parameter")
            );
        }

        let mut methods = Vec::new();
        let mut defaults = Vec::new();
        let mut index = self
            .index_after_token(&children, Token::Where)
            .unwrap_or(children.len());

        loop {
            self.skip_body_separators(&children, &mut index);
            if self.body_is_done(&children, index) {
                break;
            }

            if matches!(
                children.get(index),
                Some(NodeOrToken::Token(token)) if self.cst.token(*token).kind == Token::Default
            ) {
                defaults.push(self.lower_instance_method_entry(node, &children, &mut index, true)?);
            } else {
                methods.push(self.lower_class_method_entry(node, &children, &mut index)?);
            }
        }

        Ok(SurfaceDefinition::ClassDef {
            name: self.symbol_span_from_token(name, node),
            ty_params,
            superclasses,
            methods,
            defaults,
            docs: None,
        })
    }

    pub(super) fn lower_instance_def(
        &mut self,
        node: CstId,
    ) -> Result<SurfaceDefinition, ParseError> {
        let children = self.children(node);
        let context = if self.node_has_token(node, Token::FatArrow) {
            self.lower_context_predicates(node)?
        } else {
            Vec::new()
        };
        let instance_index = self
            .index_after_token(&children, Token::Instance)
            .unwrap_or(0);
        let class_name = self.required_token(
            node,
            children
                .iter()
                .skip(instance_index)
                .find_map(|child| match child {
                    NodeOrToken::Token(token)
                        if self.cst.token(*token).kind == Token::UpperIdent =>
                    {
                        Some(*token)
                    }
                    _ => None,
                }),
            "expected instance class name",
        )?;

        let mut args = Vec::new();
        let mut seen_class = false;
        for child in &children {
            match child {
                NodeOrToken::Token(token) if *token == class_name => seen_class = true,
                NodeOrToken::Token(token)
                    if seen_class && self.cst.token(*token).kind == Token::Where =>
                {
                    break;
                }
                NodeOrToken::Node(ty) if seen_class && self.node_is_type_like(*ty) => {
                    args.push(self.lower_type_atom(*ty)?);
                }
                _ => {}
            }
        }

        if args.is_empty() {
            return Err(self.error_at_node(node, "instance head needs at least one type argument"));
        }

        let mut methods = Vec::new();
        let mut index = self
            .index_after_token(&children, Token::Where)
            .unwrap_or(children.len());
        loop {
            self.skip_body_separators(&children, &mut index);
            if self.body_is_done(&children, index) {
                break;
            }
            methods.push(self.lower_instance_method_entry(node, &children, &mut index, false)?);
        }

        Ok(SurfaceDefinition::InstanceDef {
            context,
            class_name: self.symbol_span_from_token(class_name, node),
            args,
            methods,
            docs: None,
        })
    }

    fn lower_class_method_entry(
        &mut self,
        owner: CstId,
        children: &[NodeOrToken],
        index: &mut usize,
    ) -> Result<ClassMethod, ParseError> {
        let doc_anchor = self.first_meaningful_child_token_from(children, *index);
        let leading_docs = doc_anchor.and_then(|token| self.doc_comment_before_token(token));
        let name = self.lower_value_name_from_children(owner, children, index)?;
        self.consume_until_token(children, index, Token::Colon);
        let ty_node = self.required_child(
            owner,
            children.iter().skip(*index).find_map(|child| match child {
                NodeOrToken::Node(ty) if self.node_is_type_like(*ty) => Some(*ty),
                _ => None,
            }),
            "expected method type",
        )?;
        *index = self.child_index_after_node(children, ty_node);
        let trailing_docs = self
            .last_meaningful_token(ty_node)
            .and_then(|token| self.doc_comment_after_token(token));

        Ok(ClassMethod {
            name,
            ty: self.lower_type(ty_node)?,
            docs: self.attached_doc_comment(leading_docs, trailing_docs),
            cst_id: owner,
        })
    }

    fn lower_instance_method_entry(
        &mut self,
        owner: CstId,
        children: &[NodeOrToken],
        index: &mut usize,
        has_default_keyword: bool,
    ) -> Result<InstanceMethod, ParseError> {
        if has_default_keyword {
            *index += 1;
        }

        let name = self.lower_value_name_from_children(owner, children, index)?;
        let mut params = Vec::new();

        loop {
            self.skip_trivia_children(children, index);
            let Some(child) = children.get(*index).copied() else {
                break;
            };
            match child {
                NodeOrToken::Token(token) if self.cst.token(token).kind == Token::Eq => {
                    *index += 1;
                    break;
                }
                NodeOrToken::Token(token)
                    if matches!(self.cst.token(token).kind, Token::Ident | Token::Underscore) =>
                {
                    params.push(self.binder_span_from_token(token, owner));
                    *index += 1;
                }
                _ => *index += 1,
            }
        }

        let body_node = self.required_child(
            owner,
            children.iter().skip(*index).find_map(|child| match child {
                NodeOrToken::Node(expr) if self.node_is_expr_like(*expr) => Some(*expr),
                _ => None,
            }),
            "expected method body",
        )?;
        *index = self.child_index_after_node(children, body_node);

        Ok(InstanceMethod {
            name,
            params,
            body: self.lower_expr(body_node)?,
            cst_id: owner,
        })
    }

    fn index_after_token(&self, children: &[NodeOrToken], kind: Token) -> Option<usize> {
        children.iter().position(|child| {
            matches!(child, NodeOrToken::Token(token) if self.cst.token(*token).kind == kind)
        })
        .map(|index| index + 1)
    }

    fn consume_until_token(&self, children: &[NodeOrToken], index: &mut usize, kind: Token) {
        while let Some(child) = children.get(*index) {
            *index += 1;
            if matches!(child, NodeOrToken::Token(token) if self.cst.token(*token).kind == kind) {
                break;
            }
        }
    }

    fn child_index_after_node(&self, children: &[NodeOrToken], node: CstId) -> usize {
        children
            .iter()
            .position(|child| *child == NodeOrToken::Node(node))
            .map_or(children.len(), |index| index + 1)
    }

    fn body_is_done(&self, children: &[NodeOrToken], index: usize) -> bool {
        match children.get(index) {
            None => true,
            Some(NodeOrToken::Token(token)) => {
                matches!(self.cst.token(*token).kind, Token::VClose | Token::Eof)
            }
            Some(NodeOrToken::Node(_)) => false,
        }
    }

    fn first_meaningful_child_token_from(
        &self,
        children: &[NodeOrToken],
        start: usize,
    ) -> Option<TokenId> {
        children.iter().skip(start).find_map(|child| match child {
            NodeOrToken::Token(token) if self.token_is_meaningful(*token) => Some(*token),
            NodeOrToken::Node(node) => self.first_meaningful_token(*node),
            _ => None,
        })
    }
}
