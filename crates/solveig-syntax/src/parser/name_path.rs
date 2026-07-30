use super::*;

impl Parser<'_> {
    pub(super) fn parse_name_path(&mut self) -> Result<S<NameRef>, ParseError> {
        self.node(SyntaxKind::NamePath, |this| {
            let start = this.current_start();

            let first = match this.current_kind() {
                Token::Ident | Token::UpperIdent => {
                    let token = this.advance();
                    this.intern(this.token_source_text(&token))
                }
                _ => {
                    return Err(
                        this.error(format!("expected name, got {}", this.describe_current()))
                    );
                }
            };

            let mut segments = vec![first];
            while this.current_dot_is_qualified_separator() {
                match this.peek_kind(1) {
                    Some(Token::Ident | Token::UpperIdent) => {
                        this.advance();
                        let token = this.advance();
                        let name = this.intern(this.token_source_text(&token));
                        segments.push(name);
                    }
                    _ => break,
                }
            }

            Ok(this.sp(
                NameRef { segments },
                start,
                this.tokens[this.pos.saturating_sub(1)].end,
            ))
        })
    }

    pub(super) fn current_dot_is_qualified_separator(&self) -> bool {
        if !self.at(&Token::Dot) || self.pos == 0 {
            return false;
        }

        let dot_index = self.current_index();
        let Some(previous_index) = self.previous_significant_index(dot_index) else {
            return false;
        };
        let Some(next_index) = self.significant_index_from(dot_index + 1, 0) else {
            return false;
        };
        let previous = &self.tokens[previous_index];
        let dot = &self.tokens[dot_index];
        let next = &self.tokens[next_index];

        previous.end == dot.start
            && dot.end == next.start
            && matches!(next.kind, Token::Ident | Token::UpperIdent)
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
