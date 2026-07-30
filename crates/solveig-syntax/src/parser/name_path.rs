//! Qualified name path parsing.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_name_path(&mut self) -> Result<NamePathFacts, ParseError> {
        self.node(SyntaxKind::NamePath, |parser| {
            let start = parser.current_start();
            let first = match parser.current_kind() {
                Token::Ident | Token::UpperIdent => parser.advance(),
                _ => {
                    return Err(
                        parser.error(format!("expected name, got {}", parser.describe_current()))
                    );
                }
            };

            let mut end = first.end();
            let mut last_segment = parser.token_source_text(&first).to_string();
            let mut segment_count = 1usize;

            while parser.current_dot_is_qualified_separator() {
                match parser.peek_kind(1) {
                    Some(Token::Ident | Token::UpperIdent) => {
                        parser.advance();
                        let segment = parser.advance();
                        end = segment.end();
                        last_segment = parser.token_source_text(&segment).to_string();
                        segment_count += 1;
                    }
                    _ => break,
                }
            }

            let text = parser.source.get(start..end).unwrap_or("").to_string();
            Ok(NamePathFacts {
                text,
                last_segment,
                segment_count,
            })
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

        previous.end() == dot.start()
            && dot.end() == next.start()
            && matches!(next.kind, Token::Ident | Token::UpperIdent)
    }
}
