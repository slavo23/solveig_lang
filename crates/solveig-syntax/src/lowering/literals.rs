//! Literal cooking for the lowered surface AST.

use super::*;

fn cook_string_literal(slice: &str) -> Option<String> {
    let body = slice.strip_prefix('"')?.strip_suffix('"')?;
    decode_escaped_body(body)
}

fn cook_char_literal(slice: &str) -> Option<char> {
    let body = slice.strip_prefix('\'')?.strip_suffix('\'')?;
    let decoded = decode_escaped_body(body)?;
    let mut chars = decoded.chars();
    let ch = chars.next()?;

    if chars.next().is_none() {
        Some(ch)
    } else {
        None
    }
}

fn decode_escaped_body(body: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = body.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            out.push(decode_escape(&mut chars)?);
        } else {
            out.push(ch);
        }
    }

    Some(out)
}

fn decode_escape(chars: &mut std::str::Chars<'_>) -> Option<char> {
    match chars.next()? {
        'n' => Some('\n'),
        't' => Some('\t'),
        '\\' => Some('\\'),
        '\'' => Some('\''),
        '"' => Some('"'),
        'u' => decode_unicode_escape(chars),
        _ => None,
    }
}

fn decode_unicode_escape(chars: &mut std::str::Chars<'_>) -> Option<char> {
    if chars.next()? != '{' {
        return None;
    }

    let mut value = String::new();

    loop {
        let ch = chars.next()?;
        if ch == '}' {
            break;
        }

        if !ch.is_ascii_hexdigit() || value.len() == 6 {
            return None;
        }

        value.push(ch);
    }

    if value.is_empty() {
        return None;
    }

    let scalar = u32::from_str_radix(&value, 16).ok()?;
    char::from_u32(scalar)
}

impl CstToAst<'_> {
    fn literal_error(&self, token: TokenId) -> ParseError {
        ParseError {
            span: self.token_file_span(token),
            message: format!("unexpected character '{}'", self.token_text(token)),
        }
    }

    fn push_literal_error(&mut self, token: TokenId) -> ParseError {
        let error = self.literal_error(token);
        self.diagnostics.push(error.to_diagnostic());
        error
    }

    pub(super) fn lower_int_literal(&mut self, token: TokenId) -> Result<i64, ParseError> {
        self.token_text(token)
            .parse::<i64>()
            .map_err(|_err| self.push_literal_error(token))
    }

    pub(super) fn lower_float_literal(&mut self, token: TokenId) -> Result<f64, ParseError> {
        self.token_text(token)
            .parse::<f64>()
            .map_err(|_err| self.push_literal_error(token))
    }

    pub(super) fn lower_char_literal(&mut self, token: TokenId) -> Result<char, ParseError> {
        cook_char_literal(self.token_text(token)).ok_or_else(|| self.push_literal_error(token))
    }

    pub(super) fn lower_string_literal(&mut self, token: TokenId) -> Result<String, ParseError> {
        cook_string_literal(self.token_text(token)).ok_or_else(|| self.push_literal_error(token))
    }
}
