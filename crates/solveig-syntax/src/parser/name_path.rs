use super::*;

impl Parser<'_> {
    pub(super) fn parse_name_path(&mut self) -> Result<S<NameRef>, ParseError> {
        let start = self.current_start();

        let first = match self.current_kind() {
            Token::Ident(name) | Token::UpperIdent(name) => {
                let name = *name;
                self.advance();
                name
            }
            _ => return Err(self.error(format!("expected name, got {}", self.describe_current()))),
        };

        let mut segments = vec![first];
        while self.current_dot_is_qualified_separator() {
            match self.tokens.get(self.pos + 1).map(|token| &token.kind) {
                Some(Token::Ident(name)) | Some(Token::UpperIdent(name)) => {
                    let name = *name;
                    self.advance();
                    self.advance();
                    segments.push(name);
                }
                _ => break,
            }
        }

        Ok(sp(
            NameRef { segments },
            start,
            self.tokens[self.pos.saturating_sub(1)].end,
        ))
    }

    pub(super) fn current_dot_is_qualified_separator(&self) -> bool {
        if !self.at(&Token::Dot) || self.pos == 0 {
            return false;
        }

        let previous = &self.tokens[self.pos - 1];
        let dot = self.current();
        let Some(next) = self.tokens.get(self.pos + 1) else {
            return false;
        };

        previous.end == dot.start
            && dot.end == next.start
            && matches!(next.kind, Token::Ident(_) | Token::UpperIdent(_))
    }

    pub(super) fn is_case_block_follower(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::RParen
                | Token::RBrack
                | Token::Comma
                | Token::Then
                | Token::Else
                | Token::In
                | Token::BindArrow
                | Token::Semicolon
                | Token::VSemi
                | Token::VClose
                | Token::Eof
        )
    }
}
