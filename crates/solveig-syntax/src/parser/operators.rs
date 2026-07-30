//! Operator-name parsing helpers.

use super::*;

impl Parser<'_> {
    pub(super) fn starts_backticked_operator_name(&self) -> bool {
        self.at(&Token::Backtick)
            && matches!(self.peek_kind(1), Some(Token::Ident))
            && matches!(self.peek_kind(2), Some(Token::Backtick))
    }

    pub(super) fn parse_backticked_operator_name(&mut self) -> Result<(), ParseError> {
        self.node(SyntaxKind::BacktickOperator, |parser| {
            parser.expect(&Token::Backtick)?;
            parser.expect_ident_span()?;
            parser.expect(&Token::Backtick)?;
            Ok(())
        })
    }

    pub(super) fn parse_infix_operator_span(
        &mut self,
        allow_cons_syntax: bool,
    ) -> Result<(), ParseError> {
        if self.starts_backticked_operator_name() {
            self.parse_backticked_operator_name()
        } else {
            self.expect_operator_span(allow_cons_syntax)
        }
    }
}
