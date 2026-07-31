use super::*;

impl CstToAst<'_> {
    pub(super) fn lower_name_path(&self, node: CstId) -> Result<Spanned<NameRef>, ParseError> {
        let mut segments = Vec::new();
        let mut start = None;
        let mut end = None;

        for token in self.direct_meaningful_tokens(node) {
            if !matches!(self.cst.token(token).kind, Token::Ident | Token::UpperIdent) {
                continue;
            }

            let source_token = self.cst.token(token);
            start.get_or_insert(source_token.start());
            end = Some(source_token.end());
            segments.push(self.symbol_from_token(token));
        }

        let start = start.ok_or_else(|| self.error_at_node(node, "expected name"))?;
        let end = end.unwrap_or(start);
        Ok(sp(NameRef { segments }, start, end, node))
    }

    pub(super) fn name_ref_source_text(&self, name_ref: &Spanned<NameRef>) -> &str {
        self.spanned_source_text(name_ref)
    }

    pub(super) fn name_ref_last_source_text(&self, name_ref: &Spanned<NameRef>) -> &str {
        self.name_ref_source_text(name_ref)
            .rsplit('.')
            .next()
            .unwrap_or("")
    }
}
