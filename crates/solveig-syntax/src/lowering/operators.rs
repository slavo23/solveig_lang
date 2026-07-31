use super::*;

impl CstToAst<'_> {
    pub(super) fn fixed_operator_text(token: &Token) -> Option<&'static str> {
        match token {
            Token::Plus => Some("+"),
            Token::Minus => Some("-"),
            Token::Star => Some("*"),
            Token::Slash => Some("/"),
            Token::Percent => Some("%"),
            Token::EqEq => Some("=="),
            Token::Less => Some("<"),
            Token::Greater => Some(">"),
            Token::LessEq => Some("<="),
            Token::GreaterEq => Some(">="),
            Token::Cons => Some("::"),
            Token::Dot => Some("."),
            _ => None,
        }
    }

    pub(super) fn operator_symbol_from_token(
        &self,
        token: TokenId,
        allow_cons_syntax: bool,
    ) -> Result<Symbol, ParseError> {
        let source_token = self.cst.token(token);
        let (text, is_cons_syntax) = match source_token.kind {
            Token::Operator => (self.token_text(token), false),
            ref kind => {
                let Some(text) = Self::fixed_operator_text(kind) else {
                    return Err(self.error_at_token(
                        token,
                        format!("expected operator, got {}", self.describe_token(token)),
                    ));
                };
                (text, matches!(kind, Token::Cons))
            }
        };

        if is_cons_syntax && !allow_cons_syntax {
            return Err(self.error_at_token(
                token,
                "list cons `::` is reserved syntax, not an operator name",
            ));
        }

        if text.starts_with(':') && !is_cons_syntax {
            return Err(self.error_at_token(
                token,
                "operators starting with `:` are reserved for future constructor operators",
            ));
        }

        Ok(self.intern(text))
    }

    pub(super) fn lower_backticked_operator_name(
        &self,
        node: CstId,
        constructor_name: bool,
    ) -> Result<NameSpan, ParseError> {
        let name = self
            .direct_meaningful_tokens(node)
            .into_iter()
            .find(|token| {
                matches!(
                    (constructor_name, self.cst.token(*token).kind),
                    (true, Token::UpperIdent) | (false, Token::Ident)
                )
            })
            .ok_or_else(|| self.error_at_node(node, "expected backticked operator name"))?;

        let span = self.node_span(node);
        Ok((
            self.symbol_from_token(name),
            span.start as usize,
            span.end as usize,
            node,
        ))
    }

    pub(super) fn lower_infix_operator_child(
        &self,
        parent: CstId,
        child: NodeOrToken,
        allow_cons_syntax: bool,
    ) -> Result<NameSpan, ParseError> {
        match child {
            NodeOrToken::Node(node)
                if self.cst.node(node).kind() == SyntaxKind::BacktickOperator =>
            {
                self.lower_backticked_operator_name(node, false)
            }
            NodeOrToken::Token(token) => {
                let source_token = self.cst.token(token);
                let symbol = self.operator_symbol_from_token(token, allow_cons_syntax)?;
                Ok((symbol, source_token.start(), source_token.end(), parent))
            }
            _ => Err(self.error_at_node(parent, "expected operator")),
        }
    }

    pub(super) fn child_is_operator(&self, child: NodeOrToken, allow_cons_syntax: bool) -> bool {
        match child {
            NodeOrToken::Node(node) => self.cst.node(node).kind() == SyntaxKind::BacktickOperator,
            NodeOrToken::Token(token) => {
                let kind = &self.cst.token(token).kind;
                matches!(kind, Token::Operator)
                    || Self::fixed_operator_text(kind)
                        .is_some_and(|text| allow_cons_syntax || text != "::")
            }
        }
    }
}
