//! Pattern lowering for function equations, case arms, and bindings.
//! Later checker passes lower these patterns into coverage matrices.

use super::*;

impl CstToAst<'_> {
    pub(super) fn lower_pattern(
        &mut self,
        node: CstId,
    ) -> Result<Spanned<SurfacePattern>, ParseError> {
        match self.cst.node(node).kind() {
            SyntaxKind::Pattern => self.lower_pattern_node(node),
            SyntaxKind::PatternHead => self.lower_pattern_head(node),
            SyntaxKind::PatternArg => self.lower_pattern_arg(node),
            SyntaxKind::TuplePattern => self.lower_tuple_pattern(node),
            SyntaxKind::TupleConstructor => self.lower_tuple_constructor_pattern(node),
            SyntaxKind::NamePath => self.lower_pattern_name_path(node),
            _ => Err(self.error_at_node(node, "expected pattern")),
        }
    }

    fn lower_pattern_node(&mut self, node: CstId) -> Result<Spanned<SurfacePattern>, ParseError> {
        let pattern_children = self.child_patterns(node);
        let head =
            self.required_child(node, pattern_children.first().copied(), "expected pattern")?;
        let head = self.lower_pattern(head)?;

        if let Some(backtick) = self.first_child_of_kind(node, SyntaxKind::BacktickOperator) {
            let (ctor, start, end, cst_id) = self.lower_backticked_operator_name(backtick, true)?;
            let rhs_node = self.required_child(
                node,
                pattern_children
                    .last()
                    .copied()
                    .filter(|child| *child != head.2),
                "expected pattern after constructor operator",
            )?;
            let rhs = self.lower_pattern(rhs_node)?;
            let ctor = sp(
                NameRef {
                    segments: vec![ctor],
                },
                start,
                end,
                cst_id,
            );
            let span_start = span_start(&head);
            let span_end = span_end(&rhs);
            return Ok(self.sp_between(
                node,
                SurfacePattern::Con(ctor, vec![head, rhs]),
                span_start,
                span_end,
            ));
        }

        if let Some(cons) = self
            .direct_meaningful_tokens(node)
            .into_iter()
            .find(|token| self.cst.token(*token).kind == Token::Cons)
        {
            let rhs_node = self.required_child(
                node,
                pattern_children
                    .last()
                    .copied()
                    .filter(|child| *child != head.2),
                "expected pattern after `::`",
            )?;
            let tail = self.lower_pattern(rhs_node)?;
            let source_token = self.cst.token(cons);
            return Ok(cons_pattern(
                self,
                head,
                tail,
                source_token.start(),
                source_token.end(),
                node,
            ));
        }

        Ok(head)
    }

    fn lower_pattern_head(&mut self, node: CstId) -> Result<Spanned<SurfacePattern>, ParseError> {
        if let Some(constructor) = self.first_child_of_kind(node, SyntaxKind::TupleConstructor) {
            let arity = self
                .direct_meaningful_tokens(constructor)
                .into_iter()
                .filter(|token| self.cst.token(*token).kind == Token::Comma)
                .count()
                + 1;
            let arg_nodes = self
                .child_nodes(node)
                .into_iter()
                .filter(|child| self.node_is_pattern_like(*child) && *child != constructor)
                .collect::<Vec<_>>();
            let args = arg_nodes
                .into_iter()
                .map(|child| self.lower_pattern(child))
                .collect::<Result<Vec<_>, _>>()?;
            let start = self.node_span(node).start as usize;
            let end = self.node_span(node).end as usize;
            return Ok(tuple_constructor_pattern(
                self, arity, args, start, end, node,
            ));
        }

        if let Some(name_node) = self.first_child_of_kind(node, SyntaxKind::NamePath) {
            let name_ref = self.lower_name_path(name_node)?;
            if is_ctor_like(self.name_ref_last_source_text(&name_ref)) {
                let mut args = Vec::new();
                let mut seen_name = false;
                for child in self.child_nodes(node) {
                    if child == name_node && !seen_name {
                        seen_name = true;
                        continue;
                    }
                    if seen_name && self.node_is_pattern_like(child) {
                        args.push(self.lower_pattern(child)?);
                    }
                }
                let start = span_start(&name_ref);
                let end = args.last().map_or_else(|| span_end(&name_ref), span_end);
                return Ok(self.sp_between(node, SurfacePattern::Con(name_ref, args), start, end));
            }
        }

        let child = self.required_child(
            node,
            self.child_patterns(node).first().copied(),
            "expected pattern",
        )?;
        self.lower_pattern(child)
    }

    fn lower_pattern_arg(&mut self, node: CstId) -> Result<Spanned<SurfacePattern>, ParseError> {
        let tokens = self.direct_meaningful_tokens(node);
        let start = self.node_span(node).start as usize;
        let end = self.node_span(node).end as usize;

        if self.node_has_token(node, Token::LBrack) {
            let elems = self
                .child_patterns(node)
                .into_iter()
                .map(|child| self.lower_pattern(child))
                .collect::<Result<Vec<_>, _>>()?;
            if elems.is_empty() {
                return Ok(list_nil_pattern(self, start, end, node));
            }
            return Ok(list_pattern(self, elems, start, end, node));
        }

        if self.cst.node(node).kind() == SyntaxKind::TupleConstructor {
            return self.lower_tuple_constructor_pattern(node);
        }
        if let Some(constructor) = self.first_child_of_kind(node, SyntaxKind::TupleConstructor) {
            return self.lower_tuple_constructor_pattern(constructor);
        }
        if let Some(tuple) = self.first_child_of_kind(node, SyntaxKind::TuplePattern) {
            return self.lower_tuple_pattern(tuple);
        }

        if let Some(name) = self.first_child_of_kind(node, SyntaxKind::NamePath) {
            return self.lower_pattern_name_path(name);
        }

        if self.node_has_token(node, Token::LParen) {
            let Some(inner) = self.child_patterns(node).first().copied() else {
                return Err(self.error_at_node(node, "unit pattern `()` is not supported yet"));
            };
            let inner = self.lower_pattern(inner)?;
            return Ok(self.sp_between(node, inner.0, start, end));
        }

        let first = self.required_token(node, tokens.first().copied(), "expected pattern")?;
        match self.cst.token(first).kind {
            Token::Int => {
                let value = self.lower_int_literal(first)?;
                let token = self.cst.token(first);
                Ok(int_pattern(value, token.start(), token.end(), node))
            }
            Token::Float => {
                let value = self.lower_float_literal(first)?;
                let token = self.cst.token(first);
                Ok(literal_pattern(
                    SurfaceLiteral::Float(value),
                    token.start(),
                    token.end(),
                    node,
                ))
            }
            Token::Char => {
                let value = self.lower_char_literal(first)?;
                let token = self.cst.token(first);
                Ok(literal_pattern(
                    SurfaceLiteral::Char(value),
                    token.start(),
                    token.end(),
                    node,
                ))
            }
            Token::Str => {
                let value = self.lower_string_literal(first)?;
                let token = self.cst.token(first);
                Ok(literal_pattern(
                    SurfaceLiteral::Str(value),
                    token.start(),
                    token.end(),
                    node,
                ))
            }
            Token::Minus => self.lower_negative_pattern(node, &tokens),
            Token::Underscore => {
                let token = self.cst.token(first);
                Ok(self.sp_between(node, SurfacePattern::Wild, token.start(), token.end()))
            }
            _ => Err(self.error_at_token(
                first,
                format!("expected pattern, got {}", self.describe_token(first)),
            )),
        }
    }

    fn lower_tuple_constructor_pattern(
        &mut self,
        node: CstId,
    ) -> Result<Spanned<SurfacePattern>, ParseError> {
        let start = self.node_span(node).start as usize;
        let end = self.node_span(node).end as usize;
        let arity = self
            .direct_meaningful_tokens(node)
            .into_iter()
            .filter(|token| self.cst.token(*token).kind == Token::Comma)
            .count()
            + 1;
        Ok(tuple_constructor_pattern(
            self,
            arity,
            Vec::new(),
            start,
            end,
            node,
        ))
    }

    fn lower_tuple_pattern(&mut self, node: CstId) -> Result<Spanned<SurfacePattern>, ParseError> {
        let start = self.node_span(node).start as usize;
        let end = self.node_span(node).end as usize;
        let elems = self
            .child_patterns(node)
            .into_iter()
            .map(|child| self.lower_pattern(child))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tuple_pattern(self, elems, start, end, node))
    }

    fn lower_negative_pattern(
        &mut self,
        node: CstId,
        tokens: &[TokenId],
    ) -> Result<Spanned<SurfacePattern>, ParseError> {
        let minus = self.required_token(node, tokens.first().copied(), "expected `-`")?;
        let number = self.required_token(
            node,
            tokens.get(1).copied(),
            "expected numeric literal after `-` in pattern",
        )?;
        let minus_token = self.cst.token(minus);
        let number_token = self.cst.token(number);

        match number_token.kind {
            Token::Int => {
                let value = self.lower_int_literal(number)?;
                let value = value.checked_neg().ok_or_else(|| {
                    self.error_at_token(number, "integer literal pattern is too small")
                })?;
                Ok(int_pattern(
                    value,
                    minus_token.start(),
                    number_token.end(),
                    node,
                ))
            }
            Token::Float => {
                let value = -self.lower_float_literal(number)?;
                Ok(literal_pattern(
                    SurfaceLiteral::Float(value),
                    minus_token.start(),
                    number_token.end(),
                    node,
                ))
            }
            _ => Err(self.error_at_token(
                number,
                format!(
                    "expected numeric literal after `-` in pattern, got {}",
                    self.describe_token(number)
                ),
            )),
        }
    }

    fn lower_pattern_name_path(&self, node: CstId) -> Result<Spanned<SurfacePattern>, ParseError> {
        let name_ref = self.lower_name_path(node)?;
        let start = span_start(&name_ref);
        let end = span_end(&name_ref);

        if is_ctor_like(self.name_ref_last_source_text(&name_ref)) {
            Ok(self.sp_between(node, SurfacePattern::Con(name_ref, vec![]), start, end))
        } else if name_ref.0.segments.len() == 1 {
            Ok(self.sp_between(node, SurfacePattern::Var(name_ref.0.last()), start, end))
        } else {
            Err(self.error_at_node(
                node,
                format!(
                    "qualified variable patterns are not supported: {}",
                    self.name_ref_source_text(&name_ref)
                ),
            ))
        }
    }
}
