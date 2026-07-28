//! Recursive-descent parser for Solveig modules.
//! Child parser modules split declarations, expressions, patterns, and types.

use std::cell::Ref;

use solveig_base::{Diagnostic, FileId, FileSpan, IntoDiagnostic, Label, Session, Span, Symbol};

use crate::ast::*;
use crate::lexer::{self, LexError, PosToken, Token};
use crate::syntax::{Event, SyntaxKind};

type S<T> = (T, Span);

type NameSpan = (Symbol, usize, usize);

type LambdaParam = (S<Symbol>, Option<S<SurfaceTy>>);

fn mk_span(start: usize, end: usize) -> Span {
    Span::from_usize_range(start..end)
}

fn sp<T>(value: T, start: usize, end: usize) -> S<T> {
    (value, mk_span(start, end))
}

fn span_start<T>(spanned: &S<T>) -> usize {
    spanned.1.start as usize
}

fn span_end<T>(spanned: &S<T>) -> usize {
    spanned.1.end as usize
}

fn merge_span<T, U>(start: &S<T>, end: &S<U>) -> (usize, usize) {
    (span_start(start), span_end(end))
}

fn is_ctor_like(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn qref(session: &Session, segments: &[&str], start: usize, end: usize) -> S<NameRef> {
    sp(
        NameRef {
            segments: segments
                .iter()
                .map(|segment| session.intern(segment))
                .collect(),
        },
        start,
        end,
    )
}

fn list_nil_pattern(session: &Session, start: usize, end: usize) -> S<SurfacePattern> {
    sp(
        SurfacePattern::Con(
            qref(session, &["prelude", "list", "Nil"], start, end),
            vec![],
        ),
        start,
        end,
    )
}

fn list_nil_expr(session: &Session, start: usize, end: usize) -> S<SurfaceExpr> {
    sp(
        SurfaceExpr::Con(qref(session, &["prelude", "list", "Nil"], start, end)),
        start,
        end,
    )
}

fn list_ty(session: &Session, elem_ty: S<SurfaceTy>, start: usize, end: usize) -> S<SurfaceTy> {
    let list_con = sp(
        SurfaceTy::Con(qref(session, &["prelude", "list", "List"], start, start)),
        start,
        start,
    );
    sp(
        SurfaceTy::App(Box::new(list_con), Box::new(elem_ty)),
        start,
        end,
    )
}

fn int_pattern(value: i64, start: usize, end: usize) -> S<SurfacePattern> {
    sp(SurfacePattern::Lit(SurfaceLiteral::Int(value)), start, end)
}

fn literal_pattern(literal: SurfaceLiteral, start: usize, end: usize) -> S<SurfacePattern> {
    sp(SurfacePattern::Lit(literal), start, end)
}

fn cons_pattern(
    session: &Session,
    head: S<SurfacePattern>,
    tail: S<SurfacePattern>,
) -> S<SurfacePattern> {
    let (start, end) = merge_span(&head, &tail);
    sp(
        SurfacePattern::Con(
            qref(session, &["prelude", "list", "Cons"], start, start),
            vec![head, tail],
        ),
        start,
        end,
    )
}

fn list_pattern(
    session: &Session,
    elems: Vec<S<SurfacePattern>>,
    _start: usize,
    end: usize,
) -> S<SurfacePattern> {
    elems
        .into_iter()
        .rev()
        .fold(list_nil_pattern(session, end, end), |acc, elem| {
            cons_pattern(session, elem, acc)
        })
}

fn cons_expr(
    session: &Session,
    head: S<SurfaceExpr>,
    tail: S<SurfaceExpr>,
    start: usize,
    end: usize,
) -> S<SurfaceExpr> {
    let cons = sp(
        SurfaceExpr::Con(qref(session, &["prelude", "list", "Cons"], start, start)),
        start,
        start,
    );
    let app = sp(SurfaceExpr::App(Box::new(cons), Box::new(head)), start, end);
    sp(SurfaceExpr::App(Box::new(app), Box::new(tail)), start, end)
}

fn list_expr(
    session: &Session,
    elems: Vec<S<SurfaceExpr>>,
    _start: usize,
    end: usize,
) -> S<SurfaceExpr> {
    elems
        .into_iter()
        .rev()
        .fold(list_nil_expr(session, end, end), |acc, elem| {
            let start = span_start(&elem);
            let end = span_end(&acc);
            cons_expr(session, elem, acc, start, end)
        })
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

pub struct Parser<'a> {
    session: &'a Session,
    file: FileId,
    tokens: Vec<PosToken>,
    events: Vec<Event>,
    pos: usize,
}

#[derive(Clone, Copy)]
struct ParserCheckpoint {
    pos: usize,
    events_len: usize,
}

impl<'a> Parser<'a> {
    pub fn new(session: &'a Session, file: FileId, tokens: Vec<PosToken>) -> Self {
        Self {
            session,
            file,
            tokens,
            events: Vec::new(),
            pos: 0,
        }
    }

    fn checkpoint(&self) -> ParserCheckpoint {
        ParserCheckpoint {
            pos: self.pos,
            events_len: self.events.len(),
        }
    }

    fn restore(&mut self, checkpoint: ParserCheckpoint) {
        self.pos = checkpoint.pos;
        self.events.truncate(checkpoint.events_len);
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
        self.tokens[..before.min(self.tokens.len())]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, token)| !token.kind.is_trivia())
            .map(|(index, _)| index)
    }

    fn peek_kind(&self, offset: usize) -> Option<&Token> {
        self.significant_index_from(self.pos, offset)
            .map(|index| &self.tokens[index].kind)
    }

    fn emit_remaining_tokens(&mut self) {
        while self.pos < self.tokens.len() {
            self.events.push(Event::Token(self.pos));
            self.pos += 1;
        }
    }

    fn current(&self) -> &PosToken {
        &self.tokens[self.current_index()]
    }

    fn current_kind(&self) -> &Token {
        &self.current().kind
    }

    fn current_start(&self) -> usize {
        self.current().start
    }

    fn at(&self, token: &Token) -> bool {
        std::mem::discriminant(self.current_kind()) == std::mem::discriminant(token)
    }

    fn at_eof(&self) -> bool {
        matches!(self.current_kind(), Token::Eof)
    }

    fn advance(&mut self) -> PosToken {
        if self.pos >= self.tokens.len() {
            return self
                .tokens
                .last()
                .expect("ICE: parser token stream must contain EOF")
                .clone();
        }

        let index = self.current_index();
        for token_index in self.pos..=index {
            self.events.push(Event::Token(token_index));
        }
        self.pos = index + 1;

        self.tokens[index].clone()
    }

    fn current_span(&self) -> FileSpan {
        let token = self.current();
        FileSpan::new(self.file, mk_span(token.start, token.end))
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            span: self.current_span(),
            message: message.into(),
        }
    }

    fn push_error_diagnostic(&mut self, error: &ParseError) -> Diagnostic {
        let diagnostic = error.to_diagnostic();
        self.session.diagnostics().push(diagnostic.clone());
        diagnostic
    }

    fn token_text(&self, symbol: Symbol) -> Ref<'_, str> {
        self.session.str(symbol)
    }

    fn describe_current(&self) -> String {
        match self.current_kind() {
            Token::Ident(symbol) => format!("identifier `{}`", self.token_text(*symbol)),
            Token::UpperIdent(symbol) => format!("constructor `{}`", self.token_text(*symbol)),
            Token::Operator(symbol) => format!("operator `{}`", self.token_text(*symbol)),
            Token::ReservedOp(symbol) => {
                format!("reserved operator `{}`", self.token_text(*symbol))
            }
            Token::Int(value) => format!("integer literal `{value}`"),
            Token::Float(value) => format!("float literal `{value}`"),
            Token::Char(value) => format!("char literal `{value}`"),
            Token::Str(_) => "string literal".to_string(),
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

    fn expect_ident(&mut self) -> Result<NameSpan, ParseError> {
        match self.current_kind() {
            Token::Ident(name) => {
                let name = *name;
                let token = self.advance();
                Ok((name, token.start, token.end))
            }
            _ => Err(self.error(format!(
                "expected identifier, got {}",
                self.describe_current()
            ))),
        }
    }

    fn expect_upper(&mut self) -> Result<NameSpan, ParseError> {
        match self.current_kind() {
            Token::UpperIdent(name) => {
                let name = *name;
                let token = self.advance();
                Ok((name, token.start, token.end))
            }
            _ => Err(self.error(format!(
                "expected constructor, got {}",
                self.describe_current()
            ))),
        }
    }

    fn expect_module_name_segment(&mut self) -> Result<Symbol, ParseError> {
        match self.current_kind() {
            Token::Ident(name) | Token::UpperIdent(name) => {
                let name = *name;
                self.advance();
                Ok(name)
            }
            _ => Err(self.error(format!(
                "expected module name, got {}",
                self.describe_current()
            ))),
        }
    }

    fn fixed_operator_text(token: &Token) -> Option<&'static str> {
        match token {
            Token::Plus => Some("+"),
            Token::Minus => Some("-"),
            Token::Star => Some("*"),
            Token::Slash => Some("/"),
            Token::Percent => Some("%"),
            Token::EqEq => Some("=="),
            Token::Less => Some("<"),
            Token::Greater => Some(">"),
            Token::LessEq => Some("<="),
            Token::GreaterEq => Some(">="),
            Token::Cons => Some("::"),
            Token::Dot => Some("."),
            _ => None,
        }
    }

    fn current_operator_symbol(&self) -> Option<(Symbol, bool)> {
        match self.current_kind() {
            Token::Operator(symbol) => Some((*symbol, false)),
            token => Self::fixed_operator_text(token)
                .map(|text| (self.session.intern(text), matches!(token, Token::Cons))),
        }
    }

    fn is_infix_operator(&self) -> bool {
        self.current_operator_symbol().is_some()
    }

    fn expect_operator_symbol(&mut self, allow_cons_syntax: bool) -> Result<NameSpan, ParseError> {
        let Some((symbol, is_cons_syntax)) = self.current_operator_symbol() else {
            return Err(self.error(format!(
                "expected operator, got {}",
                self.describe_current()
            )));
        };

        if is_cons_syntax && !allow_cons_syntax {
            return Err(self.error("list cons `::` is reserved syntax, not an operator name"));
        }

        let is_reserved_constructor_operator = {
            let text = self.token_text(symbol);
            text.starts_with(':') && !is_cons_syntax
        };

        if is_reserved_constructor_operator {
            return Err(self.error(
                "operators starting with `:` are reserved for future constructor operators",
            ));
        }

        let token = self.advance();
        Ok((symbol, token.start, token.end))
    }

    fn parse_parenthesized_operator_name(&mut self) -> Result<NameSpan, ParseError> {
        let start = self.expect(&Token::LParen)?.start;
        let (symbol, _, _) = self.expect_operator_symbol(false)?;
        let end = self.expect(&Token::RParen)?.end;
        Ok((symbol, start, end))
    }

    fn starts_parenthesized_operator_name(&self) -> bool {
        if !self.at(&Token::LParen) {
            return false;
        }

        self.peek_kind(1).is_some_and(|kind| match kind {
            Token::Operator(_) => true,
            other => Self::fixed_operator_text(other)
                .is_some_and(|text| text != "::" && !text.starts_with(':')),
        }) && self
            .peek_kind(2)
            .is_some_and(|kind| matches!(kind, Token::RParen))
    }

    fn expect_value_name(&mut self) -> Result<NameSpan, ParseError> {
        if self.starts_parenthesized_operator_name() {
            self.parse_parenthesized_operator_name()
        } else {
            self.expect_ident()
        }
    }

    fn parse_import_name(&mut self) -> Result<Symbol, ParseError> {
        if self.starts_parenthesized_operator_name() {
            Ok(self.parse_parenthesized_operator_name()?.0)
        } else {
            self.expect_module_name_segment()
        }
    }

    fn expect_binder(&mut self) -> Result<NameSpan, ParseError> {
        match self.current_kind() {
            Token::Ident(name) => {
                let name = *name;
                let token = self.advance();
                Ok((name, token.start, token.end))
            }
            Token::Underscore => {
                let underscore = self.session.intern("_");
                let token = self.advance();
                Ok((underscore, token.start, token.end))
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
        self.tokens[..self.current_index().min(self.tokens.len())]
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

mod decl;
mod expr;
mod module;
mod name_path;
mod pattern;
#[cfg(test)]
mod tests;
mod ty;
pub fn parse_source(session: &Session, file: FileId, source: &str) -> ParsedModule {
    let tokens = match lexer::lex(session, file, source) {
        Ok(tokens) => tokens,
        Err(error) => {
            let error = ParseError::from(error);
            let diagnostic = error.to_diagnostic();
            session.diagnostics().push(diagnostic.clone());

            return ParsedModule {
                file,
                tokens: Vec::new(),
                events: vec![
                    Event::StartNode(SyntaxKind::Module),
                    Event::Error(diagnostic),
                    Event::FinishNode,
                ],
                imports: Vec::new(),
                defs: Vec::new(),
            };
        }
    };

    Parser::new(session, file, tokens).parse_module()
}
