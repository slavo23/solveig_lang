//! Type and predicate lowering for annotations, data fields, and class heads.
//! Kind inference consumes these syntax nodes after name resolution.

use super::*;

impl CstToAst<'_> {
    pub(super) fn lower_type(&mut self, node: CstId) -> Result<Spanned<SurfaceTy>, ParseError> {
        match self.cst.node(node).kind() {
            SyntaxKind::Type => self.lower_type_node(node),
            SyntaxKind::QualifiedType => self.lower_qualified_type(node),
            SyntaxKind::TypeArrow => self.lower_type_arrow(node),
            SyntaxKind::TypeApp => self.lower_type_app(node),
            SyntaxKind::TypeAtom => self.lower_type_atom(node),
            SyntaxKind::NamePath => self.lower_type_name_path(node),
            _ => Err(self.error_at_node(node, "expected type")),
        }
    }

    fn lower_type_node(&mut self, node: CstId) -> Result<Spanned<SurfaceTy>, ParseError> {
        if !self.node_has_token(node, Token::Forall) {
            let child = self.required_child(node, self.first_type_child(node), "expected type")?;
            return self.lower_type(child);
        }

        let mut vars = Vec::new();
        let mut after_forall = false;
        let mut before_dot = true;
        let start = self
            .direct_meaningful_tokens(node)
            .into_iter()
            .find_map(|token| {
                let kind = self.cst.token(token).kind;
                if kind == Token::Forall {
                    after_forall = true;
                    Some(self.cst.token(token).start())
                } else {
                    None
                }
            })
            .unwrap_or(self.node_span(node).start as usize);

        for token in self.direct_meaningful_tokens(node) {
            match self.cst.token(token).kind {
                Token::Forall => after_forall = true,
                Token::Dot => before_dot = false,
                Token::Ident if after_forall && before_dot => {
                    vars.push(self.symbol_span_from_token(token, node));
                }
                _ => {}
            }
        }

        let body = self.required_child(node, self.last_type_child(node), "expected forall body")?;
        let body = self.lower_type(body)?;
        let end = span_end(&body);
        Ok(self.sp_between(node, SurfaceTy::ForAll(vars, Box::new(body)), start, end))
    }

    fn lower_qualified_type(&mut self, node: CstId) -> Result<Spanned<SurfaceTy>, ParseError> {
        if !self.node_has_token(node, Token::FatArrow) {
            let child = self.required_child(node, self.first_type_child(node), "expected type")?;
            return self.lower_type(child);
        }

        let preds = self.lower_context_predicates(node)?;
        let body =
            self.required_child(node, self.last_type_child(node), "expected qualified body")?;
        let body = self.lower_type(body)?;
        let start = preds.first().map_or_else(|| span_start(&body), span_start);
        let end = span_end(&body);

        Ok(self.sp_between(
            node,
            SurfaceTy::Qualified(preds, Box::new(body)),
            start,
            end,
        ))
    }

    fn lower_type_arrow(&mut self, node: CstId) -> Result<Spanned<SurfaceTy>, ParseError> {
        let children = self
            .child_nodes(node)
            .into_iter()
            .filter(|child| self.node_is_type_like(*child))
            .collect::<Vec<_>>();
        let lhs = self.required_child(node, children.first().copied(), "expected type")?;
        let lhs = self.lower_type(lhs)?;

        if !self.node_has_token(node, Token::Arrow) {
            return Ok(lhs);
        }

        let rhs = self.required_child(node, children.last().copied(), "expected arrow result")?;
        let rhs = self.lower_type(rhs)?;
        let (start, end) = merge_span(&lhs, &rhs);
        Ok(self.sp_between(
            node,
            SurfaceTy::Arr(Box::new(lhs), Box::new(rhs)),
            start,
            end,
        ))
    }

    fn lower_type_app(&mut self, node: CstId) -> Result<Spanned<SurfaceTy>, ParseError> {
        let atoms = self
            .child_nodes(node)
            .into_iter()
            .filter(|child| self.node_is_type_like(*child))
            .collect::<Vec<_>>();
        let first = self.required_child(node, atoms.first().copied(), "expected type")?;
        let mut ty = self.lower_type(first)?;

        for atom in atoms.into_iter().skip(1) {
            let arg = self.lower_type(atom)?;
            let (start, end) = merge_span(&ty, &arg);
            ty = self.sp_between(
                node,
                SurfaceTy::App(Box::new(ty), Box::new(arg)),
                start,
                end,
            );
        }

        Ok(ty)
    }

    pub(super) fn lower_type_atom(
        &mut self,
        node: CstId,
    ) -> Result<Spanned<SurfaceTy>, ParseError> {
        if self.cst.node(node).kind() == SyntaxKind::NamePath {
            return self.lower_type_name_path(node);
        }

        let start = self.node_span(node).start as usize;
        let end = self.node_span(node).end as usize;

        if self.node_has_token(node, Token::LParen) {
            let Some(inner) = self.first_type_child(node) else {
                return Ok(self.sp_between(node, SurfaceTy::Unit, start, end));
            };
            let inner = self.lower_type(inner)?;
            return Ok(self.sp_between(node, inner.0, start, end));
        }

        if self.node_has_token(node, Token::LBrack) {
            let elem =
                self.required_child(node, self.first_type_child(node), "expected list type")?;
            let elem = self.lower_type(elem)?;
            return Ok(list_ty(self, elem, start, end, node));
        }

        if let Some(name) = self.first_child_of_kind(node, SyntaxKind::NamePath) {
            return self.lower_type_name_path(name);
        }

        Err(self.error_at_node(node, "expected type"))
    }

    fn lower_type_name_path(&mut self, node: CstId) -> Result<Spanned<SurfaceTy>, ParseError> {
        let name_ref = self.lower_name_path(node)?;
        let start = span_start(&name_ref);
        let end = span_end(&name_ref);

        if is_ctor_like(self.name_ref_last_source_text(&name_ref)) {
            Ok(self.sp_between(node, SurfaceTy::Con(name_ref), start, end))
        } else if name_ref.0.segments.len() == 1 {
            Ok(self.sp_between(node, SurfaceTy::Var(name_ref.0.last()), start, end))
        } else {
            Err(self.error_at_node(
                node,
                format!(
                    "qualified type variable is not allowed: {}",
                    self.name_ref_source_text(&name_ref)
                ),
            ))
        }
    }

    pub(super) fn lower_predicate_list(
        &mut self,
        node: CstId,
    ) -> Result<Vec<Spanned<SurfacePredicate>>, ParseError> {
        if self.cst.node(node).kind() == SyntaxKind::Predicate {
            return Ok(vec![self.lower_predicate(node)?]);
        }

        let predicates = self.child_nodes_of_kind(node, SyntaxKind::Predicate);
        if predicates.is_empty() {
            return Err(self.error_at_node(node, "expected predicate"));
        }

        predicates
            .into_iter()
            .map(|predicate| self.lower_predicate(predicate))
            .collect()
    }

    pub(super) fn lower_context_predicates(
        &mut self,
        node: CstId,
    ) -> Result<Vec<Spanned<SurfacePredicate>>, ParseError> {
        if let Some(list) = self.first_child_of_kind(node, SyntaxKind::PredicateList) {
            return self.lower_predicate_list(list);
        }

        let predicates = self.child_nodes_of_kind(node, SyntaxKind::Predicate);
        if predicates.is_empty() {
            return Err(self.error_at_node(node, "expected predicate list"));
        }

        predicates
            .into_iter()
            .map(|predicate| self.lower_predicate(predicate))
            .collect()
    }

    pub(super) fn lower_predicate(
        &mut self,
        node: CstId,
    ) -> Result<Spanned<SurfacePredicate>, ParseError> {
        let name_node = self.required_child(
            node,
            self.first_child_of_kind(node, SyntaxKind::NamePath),
            "expected class name",
        )?;
        let class_name = self.lower_name_path(name_node)?;

        if !is_ctor_like(self.name_ref_last_source_text(&class_name)) {
            return Err(self.error_at_node(
                node,
                format!(
                    "expected class name, got {}",
                    self.name_ref_source_text(&class_name)
                ),
            ));
        }

        let mut args = Vec::new();
        let mut seen_name = false;
        for child in self.child_nodes(node) {
            if child == name_node && !seen_name {
                seen_name = true;
                continue;
            }
            if self.node_is_type_like(child) {
                args.push(self.lower_type(child)?);
            }
        }

        if args.is_empty() {
            return Err(
                self.error_at_node(node, "expected at least one type argument in predicate")
            );
        }

        let start = span_start(&class_name);
        let end = args.last().map_or_else(|| span_end(&class_name), span_end);
        Ok(self.sp_between(node, SurfacePredicate { class_name, args }, start, end))
    }
}
