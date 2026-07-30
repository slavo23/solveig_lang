//! Lowers the lossless CST into the symbolized surface AST.
//!
//! This stage runs after parsing, so it may use a `Session` for interning.

use solveig_base::{Diagnostic, FileId, FileSpan, Session, Span, Symbol};

use crate::ast::*;
use crate::cst::{CstId, SyntaxTree};
use crate::lexer::{self, PosToken, Token};
use crate::parser::{ParseError, ParsedCst, parse};
use crate::syntax::{Event, SyntaxKind};

use lists::{
    cons_pattern, int_pattern, list_expr, list_nil_expr, list_nil_pattern, list_pattern, list_ty,
    literal_pattern,
};

type NameSpan = (Symbol, usize, usize);

type LambdaParam = (Spanned<Symbol>, Option<Spanned<SurfaceTy>>);

fn mk_span(start: usize, end: usize) -> Span {
    Span::from_usize_range(start..end)
}

fn sp<T>(value: T, start: usize, end: usize, cst_id: CstId) -> Spanned<T> {
    (value, mk_span(start, end), cst_id)
}

fn span_start<T>(spanned: &Spanned<T>) -> usize {
    spanned.1.start as usize
}

fn span_end<T>(spanned: &Spanned<T>) -> usize {
    spanned.1.end as usize
}

fn merge_span<T, U>(start: &Spanned<T>, end: &Spanned<U>) -> (usize, usize) {
    (span_start(start), span_end(end))
}

fn is_ctor_like(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

struct AstLowerer<'a> {
    session: &'a Session,
    file: FileId,
    source: &'a str,
    tokens: Vec<PosToken>,
    cst: Option<&'a SyntaxTree>,
    events: Vec<Event>,
    diagnostics: Vec<Diagnostic>,
    pos: usize,
    emit_events: bool,
    report_diagnostics: bool,
    cst_nodes: Vec<(CstId, SyntaxKind)>,
    next_node: usize,
    node_stack: Vec<CstId>,
    last_finished_node: Option<CstId>,
}

#[derive(Clone, Copy)]
struct LowererCheckpoint {
    pos: usize,
    events_len: usize,
    next_node: usize,
    node_stack_len: usize,
    last_finished_node: Option<CstId>,
}

impl<'a> AstLowerer<'a> {
    fn from_cst(session: &'a Session, file: FileId, source: &'a str, cst: &'a SyntaxTree) -> Self {
        Self {
            session,
            file,
            source,
            tokens: cst.tokens().to_vec(),
            cst: Some(cst),
            events: Vec::new(),
            diagnostics: Vec::new(),
            pos: 0,
            emit_events: false,
            report_diagnostics: false,
            cst_nodes: cst.nodes().map(|(id, node)| (id, node.kind())).collect(),
            next_node: 0,
            node_stack: Vec::new(),
            last_finished_node: None,
        }
    }

    fn checkpoint(&self) -> LowererCheckpoint {
        LowererCheckpoint {
            pos: self.pos,
            events_len: self.events.len(),
            next_node: self.next_node,
            node_stack_len: self.node_stack.len(),
            last_finished_node: self.last_finished_node,
        }
    }

    fn restore(&mut self, checkpoint: LowererCheckpoint) {
        self.pos = checkpoint.pos;
        self.events.truncate(checkpoint.events_len);
        self.next_node = checkpoint.next_node;
        self.node_stack.truncate(checkpoint.node_stack_len);
        self.last_finished_node = checkpoint.last_finished_node;
    }

    fn start_node(&mut self, kind: SyntaxKind) -> CstId {
        let cst_id = if self.cst_nodes.is_empty() {
            CstId::new(self.next_node)
        } else {
            match self.cst_nodes.get(self.next_node).copied() {
                Some((cst_id, cst_kind)) if cst_kind == kind => {
                    self.next_node += 1;
                    cst_id
                }
                Some((cst_id, _)) => self.node_stack.last().copied().unwrap_or(cst_id),
                None => *self
                    .node_stack
                    .last()
                    .expect("ICE: AST lowering visited past CST root scope"),
            }
        };

        if self.cst_nodes.is_empty() {
            self.next_node += 1;
        }
        self.node_stack.push(cst_id);
        self.last_finished_node = None;

        if self.emit_events {
            self.events.push(Event::StartNode(kind));
        }

        cst_id
    }

    fn finish_node(&mut self) {
        let cst_id = self
            .node_stack
            .pop()
            .expect("ICE: parser finished a syntax node without starting one");
        self.last_finished_node = Some(cst_id);

        if self.emit_events {
            self.events.push(Event::FinishNode);
        }
    }

    fn current_cst_id(&self) -> CstId {
        *self
            .node_stack
            .last()
            .expect("ICE: parser built AST outside a syntax node")
    }

    fn completed_cst_id(&self) -> CstId {
        self.last_finished_node
            .expect("ICE: parser expected a completed syntax node")
    }

    fn sp<T>(&self, value: T, start: usize, end: usize) -> Spanned<T> {
        sp(value, start, end, self.current_cst_id())
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
            if self.emit_events {
                self.events.push(Event::Token(self.pos));
            }
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
            if self.emit_events {
                self.events.push(Event::Token(token_index));
            }
        }
        self.pos = index + 1;

        self.tokens[index]
    }

    fn current_span(&self) -> FileSpan {
        let token = self.current();
        FileSpan::new(self.file, mk_span(token.start(), token.end()))
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            span: self.current_span(),
            message: message.into(),
        }
    }

    fn push_error_diagnostic(&mut self, error: &ParseError) -> Diagnostic {
        let diagnostic = error.to_diagnostic();
        if self.report_diagnostics {
            self.diagnostics.push(diagnostic.clone());
        }
        diagnostic
    }

    pub(super) fn intern(&self, text: &str) -> Symbol {
        self.session.intern(text)
    }

    fn token_source_text(&self, token: &PosToken) -> &str {
        token.source_text(self.source)
    }

    fn current_source_text(&self) -> &str {
        self.token_source_text(self.current())
    }

    fn spanned_source_text<T>(&self, spanned: &Spanned<T>) -> &str {
        self.source.get(spanned.1.to_usize_range()).unwrap_or("")
    }

    fn name_ref_source_text(&self, name_ref: &Spanned<NameRef>) -> &str {
        self.spanned_source_text(name_ref)
    }

    fn name_ref_last_source_text(&self, name_ref: &Spanned<NameRef>) -> &str {
        self.name_ref_source_text(name_ref)
            .rsplit('.')
            .next()
            .unwrap_or("")
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

    fn expect_ident(&mut self) -> Result<NameSpan, ParseError> {
        match self.current_kind() {
            Token::Ident => {
                let token = self.advance();
                let name = self.intern(self.token_source_text(&token));
                Ok((name, token.start(), token.end()))
            }
            _ => Err(self.error(format!(
                "expected identifier, got {}",
                self.describe_current()
            ))),
        }
    }

    fn expect_upper(&mut self) -> Result<NameSpan, ParseError> {
        match self.current_kind() {
            Token::UpperIdent => {
                let token = self.advance();
                let name = self.intern(self.token_source_text(&token));
                Ok((name, token.start(), token.end()))
            }
            _ => Err(self.error(format!(
                "expected constructor, got {}",
                self.describe_current()
            ))),
        }
    }

    fn expect_module_name_segment(&mut self) -> Result<Symbol, ParseError> {
        match self.current_kind() {
            Token::Ident | Token::UpperIdent => {
                let token = self.advance();
                let name = self.intern(self.token_source_text(&token));
                Ok(name)
            }
            _ => Err(self.error(format!(
                "expected module name, got {}",
                self.describe_current()
            ))),
        }
    }

    fn expect_import_name_segment(&mut self) -> Result<NameSpan, ParseError> {
        match self.current_kind() {
            Token::Ident | Token::UpperIdent => {
                let token = self.advance();
                let name = self.intern(self.token_source_text(&token));
                Ok((name, token.start(), token.end()))
            }
            _ => Err(self.error(format!(
                "expected import name, got {}",
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

    fn current_operator_text(&self) -> Option<(&str, bool)> {
        match self.current_kind() {
            Token::Operator => Some((self.current_source_text(), false)),
            token => {
                Self::fixed_operator_text(token).map(|text| (text, matches!(token, Token::Cons)))
            }
        }
    }

    fn current_operator_symbol(&self) -> Option<(Symbol, bool)> {
        self.current_operator_text()
            .map(|(text, is_cons_syntax)| (self.intern(text), is_cons_syntax))
    }

    fn is_infix_operator(&self) -> bool {
        self.current_operator_symbol().is_some() || self.starts_backticked_operator_name()
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

        let text = self
            .current_operator_text()
            .map(|(text, _)| text)
            .expect("ICE: current operator text disappeared after symbol lookup");
        let is_reserved_constructor_operator = text.starts_with(':') && !is_cons_syntax;

        if is_reserved_constructor_operator {
            return Err(self.error(
                "operators starting with `:` are reserved for future constructor operators",
            ));
        }

        let token = self.advance();
        Ok((symbol, token.start(), token.end()))
    }

    fn parse_parenthesized_operator_name(&mut self) -> Result<NameSpan, ParseError> {
        let start = self.expect(&Token::LParen)?.start();
        let (symbol, _, _) = self.expect_operator_symbol(false)?;
        let end = self.expect(&Token::RParen)?.end();
        Ok((symbol, start, end))
    }

    fn starts_parenthesized_operator_name(&self) -> bool {
        if !self.at(&Token::LParen) {
            return false;
        }

        self.peek_kind(1).is_some_and(|kind| match kind {
            Token::Operator => true,
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

    fn parse_import_name(&mut self) -> Result<Spanned<Symbol>, ParseError> {
        let (symbol, start, end) = if self.starts_parenthesized_operator_name() {
            self.parse_parenthesized_operator_name()?
        } else {
            self.expect_import_name_segment()?
        };

        Ok(self.sp(symbol, start, end))
    }

    fn expect_binder(&mut self) -> Result<NameSpan, ParseError> {
        match self.current_kind() {
            Token::Ident => {
                let token = self.advance();
                let name = self.intern(self.token_source_text(&token));
                Ok((name, token.start(), token.end()))
            }
            Token::Underscore => {
                let underscore = self.intern("_");
                let token = self.advance();
                Ok((underscore, token.start(), token.end()))
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
        if self.emit_events {
            self.events.push(Event::Error(diagnostic));
        }
        self.recover_top_level();
        self.finish_node();
    }
}

mod decl;
mod docs;
mod expr;
mod lists;
mod literals;
mod module;
mod name_path;
mod operators;
mod pattern;
#[cfg(test)]
mod tests;
mod ty;

pub fn lower(session: &Session, parsed: &ParsedCst, source: &str) -> ParsedModule {
    for diagnostic in &parsed.diagnostics {
        session.diagnostics().push(diagnostic.clone());
    }

    let cst = parsed.cst.clone();
    let (imports, defs) = {
        let mut lowerer = AstLowerer::from_cst(session, parsed.file, source, &cst);
        let (imports, defs) = lowerer.parse_module_surface();
        if !lowerer.cst_nodes.is_empty() {
            assert_eq!(
                lowerer.next_node,
                lowerer.cst_nodes.len(),
                "ICE: AST lowering did not visit every CST node"
            );
        }
        (imports, defs)
    };

    ParsedModule {
        file: parsed.file,
        cst,
        imports,
        defs,
    }
}

pub fn parse_source(session: &Session, file: FileId, source: &str) -> ParsedModule {
    let tokens = match lexer::lex(file, source) {
        Ok(tokens) => tokens,
        Err(error) => {
            let error = ParseError::from(error);
            let diagnostic = error.to_diagnostic();
            session.diagnostics().push(diagnostic.clone());
            let events = vec![
                Event::StartNode(SyntaxKind::Module),
                Event::Error(diagnostic),
                Event::FinishNode,
            ];
            let cst = SyntaxTree::from_events(file, Vec::new(), &events);

            return ParsedModule {
                file,
                cst,
                imports: Vec::new(),
                defs: Vec::new(),
            };
        }
    };
    let parsed = parse(file, source, &tokens);
    lower(session, &parsed, source)
}
