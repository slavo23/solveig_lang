//! Lowers `do` blocks from CST statements into surface do-notation nodes.

use super::*;

impl CstToAst<'_> {
    pub(super) fn lower_do(&mut self, node: CstId) -> Result<Spanned<SurfaceExpr>, ParseError> {
        let mut statements = Vec::new();
        for statement in self.child_nodes(node) {
            let stmt = if self.cst.node(statement).kind() == SyntaxKind::DoStmt {
                self.lower_do_statement(statement)?
            } else if self.node_is_expr_like(statement) {
                SurfaceDoStmt::Expr(self.lower_expr(statement)?)
            } else {
                continue;
            };
            let stmt_end = match &stmt {
                SurfaceDoStmt::Bind { expr, .. }
                | SurfaceDoStmt::Let { expr, .. }
                | SurfaceDoStmt::Expr(expr) => span_end(expr),
            };
            let stmt_start = self.node_span(statement).start as usize;
            statements.push(sp(stmt, stmt_start, stmt_end, statement));
        }

        if statements.is_empty() {
            return Err(self.error_at_node(node, "do block must contain at least one statement"));
        }

        let last = statements
            .last()
            .expect("ICE: checked non-empty do block before reading final statement");
        if !matches!(last.0, SurfaceDoStmt::Expr(_)) {
            return Err(ParseError {
                span: FileSpan::new(self.file, last.1),
                message: "final do statement must be an expression".to_string(),
            });
        }

        let start = self.node_span(node).start as usize;
        let end = span_end(last);
        Ok(self.sp_between(node, SurfaceExpr::Do(statements), start, end))
    }

    fn lower_do_statement(&mut self, node: CstId) -> Result<SurfaceDoStmt, ParseError> {
        let tokens = self.direct_meaningful_tokens(node);
        if tokens
            .first()
            .is_some_and(|token| self.cst.token(*token).kind == Token::Let)
        {
            let pattern = self.required_child(
                node,
                self.child_patterns(node).first().copied(),
                "expected let pattern",
            )?;
            let expr = self.required_child(
                node,
                self.child_exprs(node).first().copied(),
                "expected let expression",
            )?;
            return Ok(SurfaceDoStmt::Let {
                pattern: self.lower_pattern(pattern)?,
                expr: self.lower_expr(expr)?,
            });
        }

        if tokens
            .iter()
            .any(|token| self.cst.token(*token).kind == Token::BindArrow)
        {
            let pattern = self.required_child(
                node,
                self.child_patterns(node).first().copied(),
                "expected bind pattern",
            )?;
            let expr = self.required_child(
                node,
                self.child_exprs(node).first().copied(),
                "expected bind expression",
            )?;
            return Ok(SurfaceDoStmt::Bind {
                pattern: self.lower_pattern(pattern)?,
                expr: self.lower_expr(expr)?,
            });
        }

        let expr = self.required_child(
            node,
            self.child_exprs(node).first().copied(),
            "expected expression statement",
        )?;
        Ok(SurfaceDoStmt::Expr(self.lower_expr(expr)?))
    }
}
