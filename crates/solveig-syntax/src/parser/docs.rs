//! Doc-comment attachment during CST-to-AST lowering.
//! Comments remain ordinary CST trivia; these routines read nearby trivia and
//! produce the owned surface `DocComment` payload.

use super::*;

impl Parser<'_> {
    fn token_source_text(&self, token: &PosToken) -> String {
        self.session
            .source_map()
            .lookup(self.file)
            .src
            .get(token.start..token.end)
            .unwrap_or("")
            .to_string()
    }

    fn doc_line_text(&self, token: &PosToken, marker: &str) -> Option<String> {
        let text = self.token_source_text(token);
        let rest = text.trim_start().strip_prefix(marker)?;
        Some(
            rest.strip_prefix(' ')
                .unwrap_or(rest)
                .trim_end()
                .to_string(),
        )
    }

    fn newline_has_blank_line(&self, token: &PosToken) -> bool {
        self.token_source_text(token)
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            > 1
    }

    pub(super) fn doc_comment_before_index(&self, anchor_index: usize) -> Option<DocComment> {
        let mut lines = Vec::new();
        let mut index = anchor_index;

        while index > 0 {
            index -= 1;
            let token = &self.tokens[index];

            match &token.kind {
                Token::Whitespace | Token::VOpen | Token::VClose | Token::VSemi => {}
                Token::Newline => {
                    if self.newline_has_blank_line(token) {
                        break;
                    }
                }
                Token::LineComment => {
                    let Some(line) = self.doc_line_text(token, "-- |") else {
                        break;
                    };
                    lines.push(line);
                }
                _ => break,
            }
        }

        if lines.is_empty() {
            return None;
        }

        lines.reverse();
        Some(DocComment::new(lines.join("\n")))
    }

    pub(super) fn doc_comment_before_current(&self) -> Option<DocComment> {
        self.doc_comment_before_index(self.current_index())
    }

    pub(super) fn doc_comment_after_previous_significant(&self) -> Option<DocComment> {
        let last_index = self.previous_significant_index(self.pos)?;
        let last_line = self.tokens[last_index].line;

        for token in &self.tokens[last_index + 1..] {
            match &token.kind {
                Token::Whitespace if token.line == last_line => {}
                Token::LineComment if token.line == last_line => {
                    return self.doc_line_text(token, "-- ^").map(DocComment::new);
                }
                _ => break,
            }
        }

        None
    }

    pub(super) fn attached_doc_comment(
        &self,
        leading: Option<DocComment>,
        trailing: Option<DocComment>,
    ) -> Option<DocComment> {
        match (leading, trailing) {
            (Some(leading), Some(trailing)) => Some(DocComment::new(format!(
                "{}\n{}",
                leading.text, trailing.text
            ))),
            (Some(doc), None) | (None, Some(doc)) => Some(doc),
            (None, None) => None,
        }
    }
}
