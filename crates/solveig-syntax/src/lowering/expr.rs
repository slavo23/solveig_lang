//! Expression lowering for Solveig terms and do-notation blocks.
//! The syntax tree produced here is still unresolved and fixity-sensitive.

use super::*;

mod do_block;

impl CstToAst<'_> {
    pub(super) fn lower_expr(&mut self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        self.lower_expr_with_left_section(node, false)
    }

    fn lower_expr_with_left_section(
        &mut self,
        node: CstId,
        allow_left_section: bool,
    ) -> Result<Spanned<SurfaceExpr>, ParseError> {
        match self.cst.node(node).kind() {
            SyntaxKind::Expr => self.lower_expr_node(node, allow_left_section),
            SyntaxKind::LambdaExpr => self.lower_lambda(node),
            SyntaxKind::LetExpr => self.lower_let(node),
            SyntaxKind::DoExpr => self.lower_do(node),
            SyntaxKind::IfExpr => self.lower_if(node),
            SyntaxKind::CaseExpr => self.lower_case(node),
            SyntaxKind::OpChain => self.lower_op_chain(node, allow_left_section),
            SyntaxKind::OpOperand => {
                let operand = self.lower_op_operand(node)?;
                Ok(self.op_chain_expr(node, operand, Vec::new()))
            }
            SyntaxKind::AppExpr => self.lower_app(node),
            SyntaxKind::AtomExpr
            | SyntaxKind::ParenExpr
            | SyntaxKind::ListExpr
            | SyntaxKind::OperatorSection
            | SyntaxKind::NamePath => self.lower_atom(node),
            _ => Err(self.error_at_node(node, "expected expression")),
        }
    }

    fn lower_expr_node(
        &mut self,
        node: CstId,
        allow_left_section: bool,
    ) -> Result<Spanned<SurfaceExpr>, ParseError> {
        let children = self.children(node);
        let colon_index = children.iter().position(|child| {
            matches!(child, NodeOrToken::Token(token) if self.cst.token(*token).kind == Token::Colon)
        });

        let expr_child = children
            .iter()
            .enumerate()
            .find(|(index, child)| {
                colon_index.is_none_or(|colon| *index < colon)
                    && matches!(child, NodeOrToken::Node(id) if self.node_is_expr_like(*id))
            })
            .and_then(|(_, child)| match child {
                NodeOrToken::Node(id) => Some(*id),
                NodeOrToken::Token(_) => None,
            });
        let expr_child = self.required_child(node, expr_child, "expected expression")?;
        let expr = self.lower_expr_with_left_section(expr_child, allow_left_section)?;

        if let Some(colon_index) = colon_index {
            let ty_child = children
                .iter()
                .enumerate()
                .find(|(index, child)| {
                    *index > colon_index
                        && matches!(child, NodeOrToken::Node(id) if self.node_is_type_like(*id))
                })
                .and_then(|(_, child)| match child {
                    NodeOrToken::Node(id) => Some(*id),
                    NodeOrToken::Token(_) => None,
                });
            let ty_child = self.required_child(node, ty_child, "expected annotation type")?;
            let ty = self.lower_type(ty_child)?;
            let start = span_start(&expr);
            let end = span_end(&ty);
            return Ok(self.sp_between(node, SurfaceExpr::Ann(Box::new(expr), ty), start, end));
        }

        Ok(expr)
    }

    fn op_chain_start(first: &SurfaceOpOperand) -> usize {
        first
            .prefix_minuses
            .first()
            .map_or_else(|| span_start(&first.expr), |span| span.start as usize)
    }

    fn op_chain_end(first: &SurfaceOpOperand, segments: &[SurfaceOpSegment]) -> usize {
        segments.last().map_or_else(
            || span_end(&first.expr),
            |segment| span_end(&segment.rhs.expr),
        )
    }

    fn op_chain_expr(
        &self,
        node: CstId,
        first: SurfaceOpOperand,
        segments: Vec<SurfaceOpSegment>,
    ) -> Spanned<SurfaceExpr> {
        if segments.is_empty() && first.prefix_minuses.is_empty() {
            first.expr
        } else {
            let start = Self::op_chain_start(&first);
            let end = Self::op_chain_end(&first, &segments);
            self.sp_between(
                node,
                SurfaceExpr::OpSeq(Box::new(first), segments),
                start,
                end,
            )
        }
    }

    fn lower_op_chain(
        &mut self,
        node: CstId,
        allow_left_section: bool,
    ) -> Result<Spanned<SurfaceExpr>, ParseError> {
        let mut first = None;
        let mut segments = Vec::new();
        let mut pending_op = None;

        for child in self.children(node) {
            if matches!(child, NodeOrToken::Token(token) if self.cst.token(token).kind.is_trivia())
            {
                continue;
            }

            if self.child_is_operator(child, true) {
                pending_op = Some(self.lower_infix_operator_child(node, child, true)?);
                continue;
            }

            let NodeOrToken::Node(operand_node) = child else {
                continue;
            };
            if !self.node_is_expr_like(operand_node) {
                continue;
            }

            let operand = self.lower_op_operand(operand_node)?;
            if let Some((op, start, end, cst_id)) = pending_op.take() {
                segments.push(SurfaceOpSegment {
                    op: sp(op, start, end, cst_id),
                    rhs: operand,
                    cst_id: node,
                });
            } else if first.is_none() {
                first = Some(operand);
            } else {
                return Err(self.error_at_node(node, "operator is missing between operands"));
            }
        }

        let first = first.ok_or_else(|| self.error_at_node(node, "expected expression"))?;
        if let Some((op, start, end, cst_id)) = pending_op {
            if allow_left_section {
                let lhs = self.op_chain_expr(node, first, segments);
                let op = sp(op, start, end, cst_id);
                let section_start = span_start(&lhs);
                return Ok(self.sp_between(
                    node,
                    SurfaceExpr::OperatorLeftSection(Box::new(lhs), op),
                    section_start,
                    end,
                ));
            }
            return Err(self.error_at_node(node, "expected expression after operator"));
        }

        Ok(self.op_chain_expr(node, first, segments))
    }

    fn lower_op_operand(&mut self, node: CstId) -> Result<SurfaceOpOperand, ParseError> {
        if self.cst.node(node).kind() != SyntaxKind::OpOperand {
            return Ok(SurfaceOpOperand {
                prefix_minuses: Vec::new(),
                expr: self.lower_expr(node)?,
                cst_id: node,
            });
        }

        let prefix_minuses = self
            .direct_meaningful_tokens(node)
            .into_iter()
            .filter(|token| self.cst.token(*token).kind == Token::Minus)
            .map(|token| self.token_span(token))
            .collect::<Vec<_>>();
        let app = self.required_child(
            node,
            self.child_nodes(node)
                .into_iter()
                .find(|child| self.node_is_expr_like(*child)),
            "expected operand",
        )?;

        Ok(SurfaceOpOperand {
            prefix_minuses,
            expr: self.lower_app(app)?,
            cst_id: node,
        })
    }

    fn lower_app(&mut self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        if self.cst.node(node).kind() != SyntaxKind::AppExpr {
            return self.lower_atom(node);
        }

        let atoms = self
            .child_nodes(node)
            .into_iter()
            .filter(|child| self.node_is_expr_like(*child))
            .collect::<Vec<_>>();
        let first = self.required_child(node, atoms.first().copied(), "expected expression")?;
        let mut func = self.lower_atom(first)?;

        for atom in atoms.into_iter().skip(1) {
            let arg = self.lower_atom(atom)?;
            let (start, end) = merge_span(&func, &arg);
            func = self.sp_between(
                node,
                SurfaceExpr::App(Box::new(func), Box::new(arg)),
                start,
                end,
            );
        }

        Ok(func)
    }

    fn lower_atom(&mut self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        match self.cst.node(node).kind() {
            SyntaxKind::NamePath => return self.lower_name_expr(node),
            SyntaxKind::ParenExpr => return self.lower_paren_expr(node),
            SyntaxKind::ListExpr => return self.lower_list_expr(node),
            SyntaxKind::OperatorSection => return self.lower_operator_section(node),
            SyntaxKind::LambdaExpr
            | SyntaxKind::LetExpr
            | SyntaxKind::DoExpr
            | SyntaxKind::IfExpr
            | SyntaxKind::CaseExpr
            | SyntaxKind::OpChain
            | SyntaxKind::Expr => return self.lower_expr(node),
            _ => {}
        }

        if let Some(name) = self.first_child_of_kind(node, SyntaxKind::NamePath) {
            return self.lower_name_expr(name);
        }
        if let Some(child) = self.child_nodes(node).into_iter().find(|child| {
            matches!(
                self.cst.node(*child).kind(),
                SyntaxKind::ParenExpr | SyntaxKind::ListExpr | SyntaxKind::OperatorSection
            )
        }) {
            return self.lower_atom(child);
        }
        if let Some(name) = self.lower_parenthesized_operator_name(node)? {
            let start = span_start(&name);
            let end = span_end(&name);
            let name_ref = sp(
                NameRef {
                    segments: vec![name.0],
                },
                start,
                end,
                name.2,
            );
            return Ok(self.sp_between(node, SurfaceExpr::Var(name_ref), start, end));
        }

        let first = self.required_token(
            node,
            self.direct_meaningful_tokens(node).first().copied(),
            "expected expression",
        )?;
        let token = self.cst.token(first);
        match token.kind {
            Token::Int => {
                let value = self.lower_int_literal(first)?;
                Ok(self.sp_between(
                    node,
                    SurfaceExpr::Literal(SurfaceLiteral::Int(value)),
                    token.start(),
                    token.end(),
                ))
            }
            Token::Float => {
                let value = self.lower_float_literal(first)?;
                Ok(self.sp_between(
                    node,
                    SurfaceExpr::Literal(SurfaceLiteral::Float(value)),
                    token.start(),
                    token.end(),
                ))
            }
            Token::Char => {
                let value = self.lower_char_literal(first)?;
                Ok(self.sp_between(
                    node,
                    SurfaceExpr::Literal(SurfaceLiteral::Char(value)),
                    token.start(),
                    token.end(),
                ))
            }
            Token::Str => {
                let value = self.lower_string_literal(first)?;
                Ok(self.sp_between(
                    node,
                    SurfaceExpr::Literal(SurfaceLiteral::Str(value)),
                    token.start(),
                    token.end(),
                ))
            }
            Token::Hole | Token::Underscore => {
                Ok(self.sp_between(node, SurfaceExpr::Hole, token.start(), token.end()))
            }
            _ => Err(self.error_at_token(
                first,
                format!("expected expression, got {}", self.describe_token(first)),
            )),
        }
    }

    fn lower_name_expr(&self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        let name_ref = self.lower_name_path(node)?;
        let start = span_start(&name_ref);
        let end = span_end(&name_ref);
        let expr = if is_ctor_like(self.name_ref_last_source_text(&name_ref)) {
            SurfaceExpr::Con(name_ref)
        } else {
            SurfaceExpr::Var(name_ref)
        };
        Ok(self.sp_between(node, expr, start, end))
    }

    fn lower_parenthesized_operator_name(
        &self,
        node: CstId,
    ) -> Result<Option<Spanned<Symbol>>, ParseError> {
        let tokens = self.direct_meaningful_tokens(node);
        if tokens.len() != 3
            || self.cst.token(tokens[0]).kind != Token::LParen
            || self.cst.token(tokens[2]).kind != Token::RParen
        {
            return Ok(None);
        }

        if !self.child_is_operator(NodeOrToken::Token(tokens[1]), false) {
            return Ok(None);
        }

        let symbol = self.operator_symbol_from_token(tokens[1], false)?;
        Ok(Some(sp(
            symbol,
            self.cst.token(tokens[0]).start(),
            self.cst.token(tokens[2]).end(),
            node,
        )))
    }

    fn lower_paren_expr(&mut self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        let start = self.node_span(node).start as usize;
        let end = self.node_span(node).end as usize;
        let Some(inner) = self.child_exprs(node).first().copied() else {
            return Ok(self.sp_between(node, SurfaceExpr::Unit, start, end));
        };
        let inner = self.lower_expr(inner)?;
        Ok(self.sp_between(node, inner.0, start, end))
    }

    fn lower_list_expr(&mut self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        let start = self.node_span(node).start as usize;
        let end = self.node_span(node).end as usize;
        let elems = self
            .child_exprs(node)
            .into_iter()
            .map(|child| self.lower_expr(child))
            .collect::<Result<Vec<_>, _>>()?;

        if elems.is_empty() {
            Ok(list_nil_expr(self, start, end, node))
        } else {
            Ok(list_expr(self, elems, start, end, node))
        }
    }

    fn lower_operator_section(&mut self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        let children = self.children(node);
        let lparen = children.iter().position(|child| {
            matches!(child, NodeOrToken::Token(token) if self.cst.token(*token).kind == Token::LParen)
        });
        let mut index = lparen.map_or(0, |index| index + 1);
        self.skip_trivia_children(&children, &mut index);

        if let Some(child) = children.get(index).copied()
            && self.child_is_operator(child, true)
        {
            let (op, op_start, op_end, cst_id) =
                self.lower_infix_operator_child(node, child, true)?;
            let rhs = children
                .iter()
                .skip(index + 1)
                .find_map(|child| match child {
                    NodeOrToken::Node(expr) if self.node_is_expr_like(*expr) => Some(*expr),
                    _ => None,
                });
            let rhs = self.required_child(node, rhs, "expected right section operand")?;
            let rhs = self.lower_expr(rhs)?;
            let start = self.node_span(node).start as usize;
            let end = self.node_span(node).end as usize;
            return Ok(self.sp_between(
                node,
                SurfaceExpr::OperatorRightSection(sp(op, op_start, op_end, cst_id), Box::new(rhs)),
                start,
                end,
            ));
        }

        let inner = self.required_child(
            node,
            self.child_exprs(node).first().copied(),
            "expected operator section",
        )?;
        let section = self.lower_expr_with_left_section(inner, true)?;
        let start = self.node_span(node).start as usize;
        let end = self.node_span(node).end as usize;

        match section.0 {
            section @ SurfaceExpr::OperatorLeftSection(_, _) => {
                Ok(self.sp_between(node, section, start, end))
            }
            _ => Err(self.error_at_node(node, "expected operator section")),
        }
    }

    fn lower_lambda(&mut self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        let children = self.children(node);
        let start = self.node_span(node).start as usize;
        let arrow_index = children.iter().position(|child| {
            matches!(child, NodeOrToken::Token(token) if self.cst.token(*token).kind == Token::FatArrow)
        });
        let arrow_index = arrow_index.unwrap_or(children.len());
        let mut params: Vec<LambdaParam> = Vec::new();
        let mut index = 0usize;

        while index < arrow_index {
            self.skip_trivia_children(&children, &mut index);
            let Some(child) = children.get(index).copied() else {
                break;
            };

            match child {
                NodeOrToken::Token(token) if self.cst.token(token).kind == Token::Fun => {
                    index += 1;
                }
                NodeOrToken::Token(token) if self.cst.token(token).kind == Token::LParen => {
                    let param_end = children
                        .iter()
                        .enumerate()
                        .skip(index + 1)
                        .find(|(_, child)| {
                            matches!(child, NodeOrToken::Token(token) if self.cst.token(*token).kind == Token::RParen)
                        })
                        .map_or(arrow_index, |(index, _)| index);
                    let name =
                        children[index + 1..param_end]
                            .iter()
                            .find_map(|child| match child {
                                NodeOrToken::Token(token)
                                    if matches!(
                                        self.cst.token(*token).kind,
                                        Token::Ident | Token::Underscore
                                    ) =>
                                {
                                    Some(*token)
                                }
                                _ => None,
                            });
                    let name = self.required_token(node, name, "expected binder")?;
                    let ty = children[index + 1..param_end]
                        .iter()
                        .find_map(|child| match child {
                            NodeOrToken::Node(ty) if self.node_is_type_like(*ty) => Some(*ty),
                            _ => None,
                        });
                    let ty = self.required_child(node, ty, "expected parameter type")?;
                    params.push((
                        self.binder_span_from_token(name, node),
                        Some(self.lower_type(ty)?),
                    ));
                    index = param_end.saturating_add(1);
                }
                NodeOrToken::Token(token)
                    if matches!(self.cst.token(token).kind, Token::Ident | Token::Underscore) =>
                {
                    params.push((self.binder_span_from_token(token, node), None));
                    index += 1;
                }
                _ => index += 1,
            }
        }

        let body = children
            .iter()
            .skip(arrow_index.saturating_add(1))
            .find_map(|child| match child {
                NodeOrToken::Node(expr) if self.node_is_expr_like(*expr) => Some(*expr),
                _ => None,
            });
        let body = self.required_child(node, body, "expected lambda body")?;
        let body = self.lower_expr(body)?;
        let end = span_end(&body);

        Ok(self.sp_between(node, SurfaceExpr::Abs(params, Box::new(body)), start, end))
    }

    fn lower_let(&mut self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        let name = self
            .direct_meaningful_tokens(node)
            .into_iter()
            .find(|token| self.cst.token(*token).kind == Token::Ident);
        let name = self.required_token(node, name, "expected let binder")?;
        let exprs = self.child_exprs(node);
        let binding = self.required_child(node, exprs.first().copied(), "expected let binding")?;
        let body = self.required_child(node, exprs.get(1).copied(), "expected let body")?;
        let binding = self.lower_expr(binding)?;
        let body = self.lower_expr(body)?;
        let start = self.node_span(node).start as usize;
        let end = span_end(&body);

        Ok(self.sp_between(
            node,
            SurfaceExpr::Let(
                self.symbol_span_from_token(name, node),
                Box::new(binding),
                Box::new(body),
            ),
            start,
            end,
        ))
    }

    fn lower_if(&mut self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        let exprs = self.child_exprs(node);
        let cond = self.required_child(node, exprs.first().copied(), "expected if condition")?;
        let then_branch =
            self.required_child(node, exprs.get(1).copied(), "expected then branch")?;
        let else_branch =
            self.required_child(node, exprs.get(2).copied(), "expected else branch")?;
        let cond = self.lower_expr(cond)?;
        let then_branch = self.lower_expr(then_branch)?;
        let else_branch = self.lower_expr(else_branch)?;
        let start = self.node_span(node).start as usize;
        let end = span_end(&else_branch);

        Ok(self.sp_between(
            node,
            SurfaceExpr::IfThenElse(Box::new(cond), Box::new(then_branch), Box::new(else_branch)),
            start,
            end,
        ))
    }

    fn lower_case(&mut self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        let scrutinee = self.required_child(
            node,
            self.child_exprs(node).first().copied(),
            "expected case scrutinee",
        )?;
        let scrutinee = self.lower_expr(scrutinee)?;
        let mut arms = Vec::new();

        for arm in self.child_nodes_of_kind(node, SyntaxKind::CaseArm) {
            arms.push(self.lower_case_arm(arm)?);
        }

        let start = self.node_span(node).start as usize;
        let end = arms.last().map_or_else(|| span_end(&scrutinee), span_end);
        Ok(self.sp_between(
            node,
            SurfaceExpr::Case(Box::new(scrutinee), arms),
            start,
            end,
        ))
    }

    fn lower_case_arm(&mut self, node: CstId) -> Result<Spanned<SurfaceMatchArm>, ParseError> {
        let pattern = self.required_child(
            node,
            self.child_patterns(node).first().copied(),
            "expected case pattern",
        )?;
        let expr = self.required_child(
            node,
            self.child_exprs(node).last().copied(),
            "expected case arm expression",
        )?;
        let pattern = self.lower_pattern(pattern)?;
        let expr = self.lower_expr(expr)?;
        let start = span_start(&pattern);
        let end = span_end(&expr);
        Ok(self.sp_between(node, SurfaceMatchArm { pattern, expr }, start, end))
    }
}
