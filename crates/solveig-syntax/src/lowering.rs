//! Lowers the lossless CST into the symbolized surface AST.
//!
//! This stage mirrors rust-analyzer's `hir_def` tree-lowering pattern: the
//! parser owns concrete syntax decisions, while lowering walks existing syntax
//! nodes and interns the user-facing names it finds.

use solveig_base::{Diagnostic, FileId, FileSpan, Session, Span, Symbol};

use crate::ast::*;
use crate::cst::{CstId, NodeOrToken, SyntaxTree, TokenId};
use crate::lexer::{self, PosToken, Token};
use crate::parser::{ParseError, ParsedCst, parse};
use crate::syntax::{Event, SyntaxKind};

use lists::{
    cons_pattern, int_pattern, list_expr, list_nil_expr, list_nil_pattern, list_pattern, list_ty,
    literal_pattern,
};
use tuples::{
    tuple_constructor_expr, tuple_constructor_pattern, tuple_constructor_ty, tuple_expr,
    tuple_pattern, tuple_ty,
};

type NameSpan = (Symbol, usize, usize, CstId);
type LambdaParam = (Spanned<SurfacePattern>, Option<Spanned<SurfaceTy>>);

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

struct CstToAst<'a> {
    session: &'a Session,
    file: FileId,
    source: &'a str,
    cst: &'a SyntaxTree,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> CstToAst<'a> {
    fn new(session: &'a Session, file: FileId, source: &'a str, cst: &'a SyntaxTree) -> Self {
        Self {
            session,
            file,
            source,
            cst,
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn intern(&self, text: &str) -> Symbol {
        self.session.intern(text)
    }

    fn sp_between<T>(&self, node: CstId, value: T, start: usize, end: usize) -> Spanned<T> {
        sp(value, start, end, node)
    }

    fn node_span(&self, node: CstId) -> Span {
        self.cst.node(node).span()
    }

    fn children(&self, node: CstId) -> Vec<NodeOrToken> {
        self.cst.children(node).to_vec()
    }

    fn child_nodes(&self, node: CstId) -> Vec<CstId> {
        self.cst
            .children(node)
            .iter()
            .filter_map(|child| match child {
                NodeOrToken::Node(id) => Some(*id),
                NodeOrToken::Token(_) => None,
            })
            .collect()
    }

    fn child_nodes_of_kind(&self, node: CstId, kind: SyntaxKind) -> Vec<CstId> {
        self.child_nodes(node)
            .into_iter()
            .filter(|child| self.cst.node(*child).kind() == kind)
            .collect()
    }

    fn first_child_of_kind(&self, node: CstId, kind: SyntaxKind) -> Option<CstId> {
        self.child_nodes(node)
            .into_iter()
            .find(|child| self.cst.node(*child).kind() == kind)
    }

    fn first_type_child(&self, node: CstId) -> Option<CstId> {
        self.child_nodes(node)
            .into_iter()
            .find(|child| self.node_is_type_like(*child))
    }

    fn last_type_child(&self, node: CstId) -> Option<CstId> {
        self.child_nodes(node)
            .into_iter()
            .rev()
            .find(|child| self.node_is_type_like(*child))
    }

    fn child_exprs(&self, node: CstId) -> Vec<CstId> {
        self.child_nodes(node)
            .into_iter()
            .filter(|child| self.node_is_expr_like(*child))
            .collect()
    }

    fn child_patterns(&self, node: CstId) -> Vec<CstId> {
        self.child_nodes(node)
            .into_iter()
            .filter(|child| self.node_is_pattern_like(*child))
            .collect()
    }

    fn node_is_type_like(&self, node: CstId) -> bool {
        matches!(
            self.cst.node(node).kind(),
            SyntaxKind::Type
                | SyntaxKind::QualifiedType
                | SyntaxKind::TypeArrow
                | SyntaxKind::TypeApp
                | SyntaxKind::TypeAtom
                | SyntaxKind::TupleType
                | SyntaxKind::TupleConstructor
                | SyntaxKind::NamePath
        )
    }

    fn node_is_expr_like(&self, node: CstId) -> bool {
        matches!(
            self.cst.node(node).kind(),
            SyntaxKind::Expr
                | SyntaxKind::LambdaExpr
                | SyntaxKind::LetExpr
                | SyntaxKind::DoExpr
                | SyntaxKind::IfExpr
                | SyntaxKind::CaseExpr
                | SyntaxKind::OpChain
                | SyntaxKind::OpOperand
                | SyntaxKind::AppExpr
                | SyntaxKind::AtomExpr
                | SyntaxKind::ParenExpr
                | SyntaxKind::TupleExpr
                | SyntaxKind::TupleConstructor
                | SyntaxKind::ListExpr
                | SyntaxKind::OperatorSection
                | SyntaxKind::NamePath
        )
    }

    fn node_is_pattern_like(&self, node: CstId) -> bool {
        matches!(
            self.cst.node(node).kind(),
            SyntaxKind::PatternBinder
                | SyntaxKind::Pattern
                | SyntaxKind::PatternHead
                | SyntaxKind::PatternArg
                | SyntaxKind::TuplePattern
                | SyntaxKind::NamePath
        )
    }

    fn node_has_token(&self, node: CstId, kind: Token) -> bool {
        self.cst.children(node).iter().any(|child| match child {
            NodeOrToken::Token(token) => self.cst.token(*token).kind == kind,
            NodeOrToken::Node(_) => false,
        })
    }

    fn direct_tokens(&self, node: CstId) -> Vec<TokenId> {
        self.cst
            .children(node)
            .iter()
            .filter_map(|child| match child {
                NodeOrToken::Token(id) => Some(*id),
                NodeOrToken::Node(_) => None,
            })
            .collect()
    }

    fn direct_meaningful_tokens(&self, node: CstId) -> Vec<TokenId> {
        self.direct_tokens(node)
            .into_iter()
            .filter(|token| self.token_is_meaningful(*token))
            .collect()
    }

    fn first_meaningful_token(&self, node: CstId) -> Option<TokenId> {
        for child in self.cst.children(node) {
            match child {
                NodeOrToken::Node(id) => {
                    if let Some(token) = self.first_meaningful_token(*id) {
                        return Some(token);
                    }
                }
                NodeOrToken::Token(id) if self.token_is_meaningful(*id) => return Some(*id),
                NodeOrToken::Token(_) => {}
            }
        }
        None
    }

    fn last_meaningful_token(&self, node: CstId) -> Option<TokenId> {
        for child in self.cst.children(node).iter().rev() {
            match child {
                NodeOrToken::Node(id) => {
                    if let Some(token) = self.last_meaningful_token(*id) {
                        return Some(token);
                    }
                }
                NodeOrToken::Token(id) if self.token_is_meaningful(*id) => return Some(*id),
                NodeOrToken::Token(_) => {}
            }
        }
        None
    }

    fn following_significant_start_after_node(&self, node: CstId) -> usize {
        let Some(last) = self.last_meaningful_token(node) else {
            return self.node_span(node).end as usize;
        };

        self.cst
            .tokens()
            .iter()
            .skip(last.index() + 1)
            .find(|token| !token.kind.is_trivia())
            .map_or(self.node_span(node).end as usize, PosToken::start)
    }

    fn token_is_meaningful(&self, token: TokenId) -> bool {
        token_is_meaningful(self.cst.token(token))
    }

    fn token_text(&self, token: TokenId) -> &str {
        self.cst.token(token).source_text(self.source)
    }

    fn token_span(&self, token: TokenId) -> Span {
        let token = self.cst.token(token);
        mk_span(token.start(), token.end())
    }

    fn token_file_span(&self, token: TokenId) -> FileSpan {
        FileSpan::new(self.file, self.token_span(token))
    }

    fn spanned_source_text<T>(&self, spanned: &Spanned<T>) -> &str {
        self.source.get(spanned.1.to_usize_range()).unwrap_or("")
    }

    fn describe_token(&self, token: TokenId) -> String {
        match self.cst.token(token).kind {
            Token::Ident => format!("identifier `{}`", self.token_text(token)),
            Token::UpperIdent => format!("constructor `{}`", self.token_text(token)),
            Token::Operator => format!("operator `{}`", self.token_text(token)),
            Token::ReservedOp => format!("reserved operator `{}`", self.token_text(token)),
            Token::Backtick => "`".to_string(),
            Token::Int => format!("integer literal `{}`", self.token_text(token)),
            Token::Float => format!("float literal `{}`", self.token_text(token)),
            Token::Char => format!("char literal `{}`", self.token_text(token)),
            Token::Str => "string literal".to_string(),
            other => format!("{other:?}"),
        }
    }

    fn error_at_token(&self, token: TokenId, message: impl Into<String>) -> ParseError {
        ParseError {
            span: self.token_file_span(token),
            message: message.into(),
        }
    }

    fn error_at_node(&self, node: CstId, message: impl Into<String>) -> ParseError {
        ParseError {
            span: FileSpan::new(self.file, self.node_span(node)),
            message: message.into(),
        }
    }

    fn record_error(&mut self, error: &ParseError) {
        self.diagnostics.push(error.to_diagnostic());
    }

    fn node_has_diagnostics(&self, node: CstId) -> bool {
        if !self.cst.node(node).diagnostics().is_empty() {
            return true;
        }

        self.child_nodes(node)
            .into_iter()
            .any(|child| self.node_has_diagnostics(child))
    }

    fn required_child(
        &self,
        node: CstId,
        child: Option<CstId>,
        message: &'static str,
    ) -> Result<CstId, ParseError> {
        child.ok_or_else(|| self.error_at_node(node, message))
    }

    fn required_token(
        &self,
        node: CstId,
        token: Option<TokenId>,
        message: &'static str,
    ) -> Result<TokenId, ParseError> {
        token.ok_or_else(|| self.error_at_node(node, message))
    }

    fn symbol_from_token(&self, token: TokenId) -> Symbol {
        self.intern(self.token_text(token))
    }

    fn symbol_span_from_token(&self, token: TokenId, cst_id: CstId) -> Spanned<Symbol> {
        let source_token = self.cst.token(token);
        sp(
            self.symbol_from_token(token),
            source_token.start(),
            source_token.end(),
            cst_id,
        )
    }

    fn binder_span_from_token(&self, token: TokenId, cst_id: CstId) -> Spanned<Symbol> {
        let source_token = self.cst.token(token);
        let symbol = if source_token.kind == Token::Underscore {
            self.intern("_")
        } else {
            self.symbol_from_token(token)
        };
        sp(symbol, source_token.start(), source_token.end(), cst_id)
    }

    fn lower_value_name_from_children(
        &self,
        node: CstId,
        children: &[NodeOrToken],
        index: &mut usize,
    ) -> Result<Spanned<Symbol>, ParseError> {
        self.skip_trivia_children(children, index);
        let Some(child) = children.get(*index).copied() else {
            return Err(self.error_at_node(node, "expected value name"));
        };

        match child {
            NodeOrToken::Token(token) if self.cst.token(token).kind == Token::Ident => {
                *index += 1;
                Ok(self.symbol_span_from_token(token, node))
            }
            NodeOrToken::Token(token) if self.cst.token(token).kind == Token::LParen => {
                let start = self.cst.token(token).start();
                *index += 1;
                self.skip_trivia_children(children, index);
                let op = self.required_child_token(children.get(*index).copied(), node)?;
                let symbol = self.operator_symbol_from_token(op, true)?;
                *index += 1;
                self.skip_trivia_children(children, index);
                let end = if let Some(NodeOrToken::Token(close)) = children.get(*index).copied()
                    && self.cst.token(close).kind == Token::RParen
                {
                    *index += 1;
                    self.cst.token(close).end()
                } else {
                    self.cst.token(op).end()
                };
                Ok(sp(symbol, start, end, node))
            }
            _ => Err(self.error_at_node(node, "expected value name")),
        }
    }

    fn lower_import_name_from_children(
        &self,
        node: CstId,
        children: &[NodeOrToken],
        index: &mut usize,
    ) -> Result<Spanned<Symbol>, ParseError> {
        self.skip_trivia_children(children, index);
        let Some(child) = children.get(*index).copied() else {
            return Err(self.error_at_node(node, "expected import name"));
        };

        match child {
            NodeOrToken::Token(token)
                if matches!(self.cst.token(token).kind, Token::Ident | Token::UpperIdent) =>
            {
                *index += 1;
                Ok(self.symbol_span_from_token(token, node))
            }
            NodeOrToken::Token(token) if self.cst.token(token).kind == Token::LParen => {
                self.lower_value_name_from_children(node, children, index)
            }
            _ => Err(self.error_at_node(node, "expected import name")),
        }
    }

    fn required_child_token(
        &self,
        child: Option<NodeOrToken>,
        node: CstId,
    ) -> Result<TokenId, ParseError> {
        match child {
            Some(NodeOrToken::Token(token)) => Ok(token),
            _ => Err(self.error_at_node(node, "expected token")),
        }
    }

    fn skip_trivia_children(&self, children: &[NodeOrToken], index: &mut usize) {
        while let Some(NodeOrToken::Token(token)) = children.get(*index) {
            if !self.cst.token(*token).kind.is_trivia() {
                break;
            }
            *index += 1;
        }
    }

    fn skip_body_separators(&self, children: &[NodeOrToken], index: &mut usize) {
        loop {
            self.skip_trivia_children(children, index);
            let Some(NodeOrToken::Token(token)) = children.get(*index) else {
                return;
            };
            if matches!(
                self.cst.token(*token).kind,
                Token::VOpen | Token::VSemi | Token::Semicolon
            ) {
                *index += 1;
            } else {
                return;
            }
        }
    }
}

fn token_is_meaningful(token: &PosToken) -> bool {
    !token.kind.is_trivia() && !token.is_synthetic() && !matches!(token.kind, Token::Eof)
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
mod tuples;
mod ty;

pub fn lower(session: &Session, parsed: &ParsedCst, source: &str) -> ParsedModule {
    for diagnostic in &parsed.diagnostics {
        session.diagnostics().push(diagnostic.clone());
    }

    let cst = parsed.cst.clone();
    let (imports, defs, diagnostics) = {
        let mut lowerer = CstToAst::new(session, parsed.file, source, &cst);
        let (imports, defs) = lowerer.lower_module_surface();
        (imports, defs, lowerer.diagnostics)
    };

    if parsed.diagnostics.is_empty() {
        for diagnostic in diagnostics {
            session.diagnostics().push(diagnostic);
        }
    }

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
