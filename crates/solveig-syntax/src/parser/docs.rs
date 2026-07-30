//! Doc-comment lowering from explicit CST comment attachments.
//! Comments remain ordinary CST trivia; the CST records which comments belong
//! to nearby source tokens, and these routines produce owned `DocComment`s.

use super::*;
use crate::cst::TokenId;

impl Parser<'_> {
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

    fn leading_doc_comment_from_tokens(&self, comments: &[TokenId]) -> Option<DocComment> {
        let mut lines = Vec::new();

        for comment in comments.iter().rev() {
            let Some(line) = self.doc_line_text(self.cst?.token(*comment), "-- |") else {
                break;
            };
            lines.push(line);
        }

        if lines.is_empty() {
            return None;
        }

        lines.reverse();
        Some(DocComment::new(lines.join("\n")))
    }

    fn trailing_doc_comment_from_tokens(&self, comments: &[TokenId]) -> Option<DocComment> {
        let comment = comments.first()?;
        self.doc_line_text(self.cst?.token(*comment), "-- ^")
            .map(DocComment::new)
    }

    pub(super) fn doc_comment_before_current(&self) -> Option<DocComment> {
        let token = TokenId::new(self.current_index());
        self.leading_doc_comment_from_tokens(self.cst?.leading_comments(token))
    }

    pub(super) fn doc_comment_after_previous_significant(&self) -> Option<DocComment> {
        let token = TokenId::new(self.previous_significant_index(self.pos)?);
        self.trailing_doc_comment_from_tokens(self.cst?.trailing_comments(token))
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
