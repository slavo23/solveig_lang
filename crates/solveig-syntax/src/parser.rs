//! Standalone recursive-descent parser for Solveig modules.
//!
//! This module consumes lexer tokens and emits a lossless CST event stream.
//! AST construction happens later, during lowering.

use solveig_base::{Diagnostic, FileId, FileSpan, IntoDiagnostic, Label, Span};

use crate::cst::SyntaxTree;
use crate::lexer::{LexError, PosToken, Token};
use crate::syntax::{Event, SyntaxKind};

mod decl;
mod expr;
mod module;
mod name_path;
mod operators;
mod pattern;
mod ty;

fn mk_span(start: usize, end: usize) -> Span {
    Span::from_usize_range(start..end)
}

fn is_ctor_like(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub span: FileSpan,
    pub message: String,
}

impl From<LexError> for ParseError {
    fn from(error: LexError) -> Self {
        Self {
            span: error.span,
            message: error.message,
        }
    }
}

impl ParseError {
    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::error("PAR0001", "parse error")
            .with_label(Label::primary(self.span, self.message.clone()))
    }
}

impl IntoDiagnostic for ParseError {
    fn into_diagnostic(self, (): ()) -> Diagnostic {
        self.to_diagnostic()
    }
}

impl IntoDiagnostic for &ParseError {
    fn into_diagnostic(self, (): ()) -> Diagnostic {
        self.to_diagnostic()
    }
}

#[derive(Clone)]
struct NamePathFacts {
    text: String,
    last_segment: String,
    segment_count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DoStatementKind {
    Bind,
    Let,
    Expr,
}

#[derive(Clone, Copy)]
struct DoStatementFacts {
    kind: DoStatementKind,
    start: usize,
    end: usize,
}

#[cfg(test)]
pub(crate) struct ParsedEvents {
    pub(crate) tokens: Vec<PosToken>,
    pub(crate) events: Vec<Event>,
}

#[derive(Clone, Debug)]
pub struct ParsedCst {
    pub file: FileId,
    pub cst: SyntaxTree,
    pub tokens: Vec<PosToken>,
    pub diagnostics: Vec<Diagnostic>,
}

struct Parser<'source> {
    file: FileId,
    source: &'source str,
    tokens: Vec<PosToken>,
    events: Vec<Event>,
    diagnostics: Vec<Diagnostic>,
    pos: usize,
}

#[derive(Clone, Copy)]
struct ParserCheckpoint {
    pos: usize,
    events_len: usize,
    diagnostics_len: usize,
}

impl<'source> Parser<'source> {
    fn new(file: FileId, source: &'source str, tokens: Vec<PosToken>) -> Self {
        Self {
            file,
            source,
            tokens,
            events: Vec::new(),
            diagnostics: Vec::new(),
            pos: 0,
        }
    }

    fn checkpoint(&self) -> ParserCheckpoint {
        ParserCheckpoint {
            pos: self.pos,
            events_len: self.events.len(),
            diagnostics_len: self.diagnostics.len(),
        }
    }

    fn restore(&mut self, checkpoint: ParserCheckpoint) {
        self.pos = checkpoint.pos;
        self.events.truncate(checkpoint.events_len);
        self.diagnostics.truncate(checkpoint.diagnostics_len);
    }

    fn start_node(&mut self, kind: SyntaxKind) {
        self.events.push(Event::StartNode(kind));
    }

    fn finish_node(&mut self) {
        self.events.push(Event::FinishNode);
    }

    fn node<T>(
        &mut self,
        kind: SyntaxKind,
        parse: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        self.start_node(kind);
        let result = parse(self);
        self.finish_node();
        result
    }

    fn node_value<T>(&mut self, kind: SyntaxKind, parse: impl FnOnce(&mut Self) -> T) -> T {
        self.start_node(kind);
        let value = parse(self);
        self.finish_node();
        value
    }

    fn significant_index_from(&self, start: usize, offset: usize) -> Option<usize> {
        let mut seen = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(start) {
            if token.kind.is_trivia() {
                continue;
            }

            if seen == offset {
                return Some(index);
            }

            seen += 1;
        }

        None
    }

    fn current_index(&self) -> usize {
        self.significant_index_from(self.pos, 0)
            .unwrap_or_else(|| self.tokens.len().saturating_sub(1))
    }

    fn previous_significant_index(&self, before: usize) -> Option<usize> {
        self.tokens_before(before)
            .iter()
            .enumerate()
            .rev()
            .find(|(_, token)| !token.kind.is_trivia())
            .map(|(index, _)| index)
    }

    fn previous_consumed_significant_token(&self) -> Option<&PosToken> {
        self.previous_significant_index(self.pos)
            .map(|index| self.token(index))
    }

    fn peek_kind(&self, offset: usize) -> Option<&Token> {
        self.significant_index_from(self.pos, offset)
            .map(|index| &self.token(index).kind)
    }

    fn emit_remaining_tokens(&mut self) {
        while self.pos < self.tokens.len() {
            self.events.push(Event::Token(self.pos));
            self.pos += 1;
        }
    }

    fn current(&self) -> &PosToken {
        self.token(self.current_index())
    }

    fn current_kind(&self) -> &Token {
        &self.current().kind
    }

    fn current_start(&self) -> usize {
        self.current().start()
    }

    fn at(&self, token: &Token) -> bool {
        std::mem::discriminant(self.current_kind()) == std::mem::discriminant(token)
    }

    fn at_eof(&self) -> bool {
        matches!(self.current_kind(), Token::Eof)
    }

    fn advance(&mut self) -> PosToken {
        if self.pos >= self.tokens.len() {
            return *self
                .tokens
                .last()
                .expect("ICE: parser token stream must contain EOF");
        }

        let index = self.current_index();
        for token_index in self.pos..=index {
            self.events.push(Event::Token(token_index));
        }
        self.pos = index + 1;

        *self.token(index)
    }

    fn current_span(&self) -> FileSpan {
        let token = self.current();
        FileSpan::new(self.file, mk_span(token.start(), token.end()))
    }

    fn token(&self, index: usize) -> &PosToken {
        self.tokens
            .get(index)
            .expect("ICE: parser token index must be in bounds")
    }

    fn tokens_before(&self, before: usize) -> &[PosToken] {
        self.tokens
            .get(..before.min(self.tokens.len()))
            .expect("ICE: parser token prefix range must be in bounds")
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            span: self.current_span(),
            message: message.into(),
        }
    }

    fn push_error_diagnostic(&mut self, error: &ParseError) -> Diagnostic {
        let diagnostic = error.to_diagnostic();
        self.diagnostics.push(diagnostic.clone());
        diagnostic
    }

    fn token_source_text(&self, token: &PosToken) -> &str {
        token.source_text(self.source)
    }

    fn current_source_text(&self) -> &str {
        self.token_source_text(self.current())
    }

    fn describe_current(&self) -> String {
        match self.current_kind() {
            Token::Ident => format!("identifier `{}`", self.current_source_text()),
            Token::UpperIdent => format!("constructor `{}`", self.current_source_text()),
            Token::Operator => format!("operator `{}`", self.current_source_text()),
            Token::ReservedOp => format!("reserved operator `{}`", self.current_source_text()),
            Token::Backtick => "`".to_string(),
            Token::Int => format!("integer literal `{}`", self.current_source_text()),
            Token::Float => format!("float literal `{}`", self.current_source_text()),
            Token::Char => format!("char literal `{}`", self.current_source_text()),
            Token::Str => "string literal".to_string(),
            other => format!("{other:?}"),
        }
    }

    fn expect(&mut self, token: &Token) -> Result<PosToken, ParseError> {
        if self.at(token) {
            Ok(self.advance())
        } else {
            Err(self.error(format!(
                "expected {:?}, got {}",
                token,
                self.describe_current()
            )))
        }
    }

    fn expect_ident_span(&mut self) -> Result<(), ParseError> {
        match self.current_kind() {
            Token::Ident => {
                self.advance();
                Ok(())
            }
            _ => Err(self.error(format!(
                "expected identifier, got {}",
                self.describe_current()
            ))),
        }
    }

    fn expect_upper_span(&mut self) -> Result<(), ParseError> {
        match self.current_kind() {
            Token::UpperIdent => {
                self.advance();
                Ok(())
            }
            _ => Err(self.error(format!(
                "expected constructor, got {}",
                self.describe_current()
            ))),
        }
    }

    fn expect_module_name_segment(&mut self) -> Result<(), ParseError> {
        match self.current_kind() {
            Token::Ident | Token::UpperIdent => {
                self.advance();
                Ok(())
            }
            _ => Err(self.error(format!(
                "expected module name, got {}",
                self.describe_current()
            ))),
        }
    }

    fn expect_import_name_segment(&mut self) -> Result<(), ParseError> {
        match self.current_kind() {
            Token::Ident | Token::UpperIdent => {
                self.advance();
                Ok(())
            }
            _ => Err(self.error(format!(
                "expected import name, got {}",
                self.describe_current()
            ))),
        }
    }

    fn special_operator_text(token: &Token) -> Option<&'static str> {
        match token {
            Token::Minus => Some("-"),
            Token::Cons => Some("::"),
            Token::Dot => Some("."),
            _ => None,
        }
    }

    fn current_operator_text(&self) -> Option<(&str, bool)> {
        match self.current_kind() {
            Token::Operator => Some((self.current_source_text(), false)),
            token => {
                Self::special_operator_text(token).map(|text| (text, matches!(token, Token::Cons)))
            }
        }
    }

    fn is_infix_operator(&self) -> bool {
        self.current_operator_text().is_some() || self.starts_backticked_operator_name()
    }

    fn expect_operator_span(&mut self, allow_cons_syntax: bool) -> Result<(), ParseError> {
        let Some((operator_text, is_cons_syntax)) = self.current_operator_text() else {
            return Err(self.error(format!(
                "expected operator, got {}",
                self.describe_current()
            )));
        };

        if is_cons_syntax && !allow_cons_syntax {
            return Err(self.error("list cons `::` is reserved syntax, not an operator name"));
        }

        if operator_text.starts_with(':') && !is_cons_syntax {
            return Err(self.error(
                "operators starting with `:` are reserved for future constructor operators",
            ));
        }

        self.advance();
        Ok(())
    }

    fn parse_parenthesized_operator_name(&mut self) -> Result<(), ParseError> {
        self.expect(&Token::LParen)?;
        self.expect_operator_span(true)?;
        self.expect(&Token::RParen)?;
        Ok(())
    }

    fn starts_parenthesized_operator_name(&self) -> bool {
        if !self.at(&Token::LParen) {
            return false;
        }

        self.peek_kind(1).is_some_and(|kind| match kind {
            Token::Operator => true,
            other => Self::special_operator_text(other)
                .is_some_and(|text| !text.starts_with(':') || text == "::"),
        }) && self
            .peek_kind(2)
            .is_some_and(|kind| matches!(kind, Token::RParen))
    }

    fn expect_value_name(&mut self) -> Result<(), ParseError> {
        if self.starts_parenthesized_operator_name() {
            self.parse_parenthesized_operator_name()
        } else {
            self.expect_ident_span()
        }
    }

    fn parse_import_name(&mut self) -> Result<(), ParseError> {
        if self.starts_parenthesized_operator_name() {
            self.parse_parenthesized_operator_name()
        } else {
            self.expect_import_name_segment()
        }
    }

    fn expect_binder_span(&mut self) -> Result<(), ParseError> {
        match self.current_kind() {
            Token::Ident | Token::Underscore => {
                self.advance();
                Ok(())
            }
            _ => Err(self.error(format!("expected binder, got {}", self.describe_current()))),
        }
    }

    fn skip_separators(&mut self) {
        while matches!(self.current_kind(), Token::VSemi | Token::Semicolon) {
            self.advance();
        }
    }

    fn is_final_module_vclose(&self) -> bool {
        if !self.at(&Token::VClose) {
            return false;
        }

        let mut offset = 0usize;
        loop {
            match self.peek_kind(offset) {
                Some(Token::VClose | Token::VSemi | Token::Semicolon) => offset += 1,
                Some(Token::Eof) | None => return true,
                Some(_) => return false,
            }
        }
    }

    fn skip_top_level_junk(&mut self) {
        loop {
            self.skip_separators();
            if self.at(&Token::VClose) && !self.is_final_module_vclose() {
                self.advance();
                continue;
            }
            break;
        }
    }

    fn layout_depth_before_current(&self) -> usize {
        self.tokens_before(self.current_index())
            .iter()
            .fold(0usize, |depth, token| match token.kind {
                Token::VOpen => depth + 1,
                Token::VClose => depth.saturating_sub(1),
                _ => depth,
            })
    }

    fn is_top_level_sync_keyword(&self) -> bool {
        matches!(
            self.current_kind(),
            Token::Data
                | Token::Type
                | Token::Sig
                | Token::Class
                | Token::Instance
                | Token::Import
                | Token::Infixl
                | Token::Infixr
                | Token::Infix
        )
    }

    fn recover_top_level(&mut self) {
        let mut depth = self.layout_depth_before_current();
        let start_pos = self.pos;

        while !self.at_eof() {
            match self.current_kind() {
                Token::VOpen => {
                    depth += 1;
                    self.advance();
                }
                Token::VClose => {
                    if depth <= 1 {
                        return;
                    }

                    depth -= 1;
                    self.advance();
                }
                Token::VSemi | Token::Semicolon if depth <= 1 => {
                    self.advance();
                    return;
                }
                _ if self.pos != start_pos && depth <= 1 && self.is_top_level_sync_keyword() => {
                    return;
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn recover_top_level_as_error_node(&mut self, error: &ParseError) {
        let diagnostic = self.push_error_diagnostic(error);
        self.start_node(SyntaxKind::Error);
        self.events.push(Event::Error(diagnostic));
        self.recover_top_level();
        self.finish_node();
    }
}

#[cfg(test)]
pub(crate) fn parse_source_events(file: FileId, source: &str) -> Result<ParsedEvents, ParseError> {
    let tokens = crate::lexer::lex(file, source)?;
    let mut parser = Parser::new(file, source, tokens);
    parser.parse_module_surface();
    Ok(ParsedEvents {
        tokens: parser.tokens,
        events: parser.events,
    })
}

pub(crate) fn parse(file: FileId, source: &str, tokens: &[PosToken]) -> ParsedCst {
    let mut parser = Parser::new(file, source, tokens.to_vec());
    parser.parse_module_surface();
    let cst = SyntaxTree::from_events(file, parser.tokens.clone(), &parser.events);
    ParsedCst {
        file,
        cst,
        tokens: parser.tokens,
        diagnostics: parser.diagnostics,
    }
}

#[cfg(test)]
pub(crate) fn parse_cst(
    file: FileId,
    source: &str,
) -> Result<(SyntaxTree, Vec<Diagnostic>), ParseError> {
    let tokens = crate::lexer::lex(file, source)?;
    let parsed = parse(file, source, &tokens);
    Ok((parsed.cst, parsed.diagnostics))
}

#[cfg(test)]
mod tests;
