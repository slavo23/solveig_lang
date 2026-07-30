use super::*;

impl Parser<'_> {
    pub(super) fn starts_backticked_operator_name(&self) -> bool {
        self.at(&Token::Backtick)
            && matches!(self.peek_kind(1), Some(Token::Ident))
            && matches!(self.peek_kind(2), Some(Token::Backtick))
    }

    pub(super) fn parse_backticked_operator_name(&mut self) -> Result<NameSpan, ParseError> {
        self.node(SyntaxKind::BacktickOperator, |this| {
            let start = this.expect(&Token::Backtick)?.start;
            let (symbol, _, _) = this.expect_ident()?;
            let end = this.expect(&Token::Backtick)?.end;
            Ok((symbol, start, end))
        })
    }

    pub(super) fn parse_infix_operator_symbol(
        &mut self,
        allow_cons_syntax: bool,
    ) -> Result<NameSpan, ParseError> {
        if self.starts_backticked_operator_name() {
            self.parse_backticked_operator_name()
        } else {
            self.expect_operator_symbol(allow_cons_syntax)
        }
    }
}
