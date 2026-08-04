//! Hand-rolled concrete syntax tree for Solveig source files.
//!
//! The parser emits flat events; this module builds the arena used by lowering
//! and later editor features. The token stream is authoritative for source text
//! reconstruction. Comment attachment lists are authoritative for which
//! declaration a documentation comment documents; consumers must not rediscover
//! documentation ownership by scanning spans.

use la_arena::{Arena, Idx, RawIdx};
use solveig_base::{Diagnostic, FileId, Span};

use crate::lexer::{PosToken, Token};
use crate::syntax::{Event, SyntaxKind};

mod rebuild;

/// Identifies a syntax node in a `SyntaxTree` arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CstId(Idx<SyntaxNode>);

impl CstId {
    pub(crate) fn new(index: usize) -> Self {
        let index = u32::try_from(index).expect("ICE: CST node arena exceeded u32 capacity");
        Self(Idx::from_raw(RawIdx::from_u32(index)))
    }

    pub fn index(self) -> usize {
        self.0.into_raw().into_u32() as usize
    }
}

/// Identifies a token in a `SyntaxTree` token stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenId(u32);

impl TokenId {
    pub(crate) fn new(index: usize) -> Self {
        Self(u32::try_from(index).expect("ICE: CST token table exceeded u32 capacity"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A child entry in a concrete syntax node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeOrToken {
    Node(CstId),
    Token(TokenId),
}

/// Stores one concrete syntax node in the tree arena.
#[derive(Clone, Debug)]
pub struct SyntaxNode {
    kind: SyntaxKind,
    span: Span,
    parent: Option<CstId>,
    children: Vec<NodeOrToken>,
    diagnostics: Vec<Diagnostic>,
}

impl SyntaxNode {
    /// Returns this node's syntax kind.
    pub fn kind(&self) -> SyntaxKind {
        self.kind
    }

    /// Returns this node's byte span.
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns this node's parent id.
    pub fn parent(&self) -> Option<CstId> {
        self.parent
    }

    /// Returns this node's ordered children.
    pub fn children(&self) -> &[NodeOrToken] {
        &self.children
    }

    /// Returns diagnostics attached while building this node.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// Owns the concrete syntax arena and token stream for one parsed file.
#[derive(Clone, Debug)]
pub struct SyntaxTree {
    file: FileId,
    root: CstId,
    nodes: Arena<SyntaxNode>,
    tokens: Vec<PosToken>,
    token_parents: Vec<Option<CstId>>,
    leading_comments: Vec<Vec<TokenId>>,
    trailing_comments: Vec<Vec<TokenId>>,
}

impl SyntaxTree {
    /// Builds a syntax tree from parser events and the full token stream.
    pub fn from_events(file: FileId, tokens: Vec<PosToken>, events: &[Event]) -> Self {
        // rust-analyzer's event parser architecture builds trees from a flat
        // StartNode/Token/FinishNode stream; Solveig keeps the arena hand-rolled.
        let mut tree = Self {
            file,
            root: CstId::new(0),
            nodes: Arena::new(),
            token_parents: vec![None; tokens.len()],
            leading_comments: vec![Vec::new(); tokens.len()],
            trailing_comments: vec![Vec::new(); tokens.len()],
            tokens,
        };
        let mut stack = Vec::new();
        let mut node_has_meaningful_token = Vec::new();
        let mut root = None;

        for event in events {
            match event {
                Event::StartNode(kind) => {
                    let parent = stack.last().copied();
                    let id = tree.push_node(*kind, parent);
                    assert_eq!(
                        node_has_meaningful_token.len(),
                        id.index(),
                        "ICE: CST builder node state must match arena order"
                    );
                    node_has_meaningful_token.push(false);

                    if let Some(parent) = parent {
                        tree.node_mut(parent).children.push(NodeOrToken::Node(id));
                    } else {
                        assert!(root.is_none(), "ICE: CST event stream has multiple roots");
                        root = Some(id);
                    }

                    stack.push(id);
                }
                Event::Token(index) => {
                    let parent = *stack
                        .last()
                        .expect("ICE: CST token event must be inside a syntax node");
                    let token_id = TokenId::new(*index);
                    let is_trivia = token_is_trivia(tree.token(token_id));
                    let is_meaningful = token_is_meaningful(tree.token(token_id));
                    let slot = tree
                        .token_parents
                        .get_mut(*index)
                        .expect("ICE: CST token event index is outside the token stream");

                    assert!(slot.is_none(), "ICE: CST token event emitted a token twice");
                    if is_trivia {
                        let (parent, before_child) =
                            trivia_placement(&stack, &node_has_meaningful_token);
                        *slot = Some(parent);
                        tree.insert_token_child(parent, token_id, before_child);
                    } else {
                        if is_meaningful {
                            mark_meaningful_tokens(&stack, &mut node_has_meaningful_token);
                        }
                        *slot = Some(parent);
                        tree.node_mut(parent)
                            .children
                            .push(NodeOrToken::Token(token_id));
                    }
                }
                Event::FinishNode => {
                    let id = stack
                        .pop()
                        .expect("ICE: CST finish event without an open node");
                    let span = tree.span_for_node(id);
                    tree.node_mut(id).span = span;
                }
                Event::Error(diagnostic) => {
                    let node = *stack
                        .last()
                        .expect("ICE: CST error event must be inside a syntax node");
                    tree.node_mut(node).diagnostics.push(diagnostic.clone());
                }
            }
        }

        assert!(stack.is_empty(), "ICE: CST event stream left nodes open");
        let root = root.expect("ICE: CST event stream must contain a root node");
        for (index, parent) in tree.token_parents.iter().enumerate() {
            assert!(
                parent.is_some(),
                "ICE: CST event stream did not place token {index}"
            );
        }

        tree.root = root;
        tree.collapse_trivial_wrappers();
        tree.recompute_node_spans();
        tree.attach_comment_trivia();
        tree.assert_span_invariants();
        tree
    }

    /// Returns this tree's file id.
    pub fn file(&self) -> FileId {
        self.file
    }

    /// Returns the root syntax node id.
    pub fn root(&self) -> CstId {
        self.root
    }

    /// Returns all tokens owned by the tree.
    pub fn tokens(&self) -> &[PosToken] {
        &self.tokens
    }

    /// Returns all arena nodes with their ids.
    pub fn nodes(&self) -> impl Iterator<Item = (CstId, &SyntaxNode)> + '_ {
        self.nodes.iter().map(|(id, node)| (CstId(id), node))
    }

    /// Returns one syntax node by id.
    pub fn node(&self, id: CstId) -> &SyntaxNode {
        &self.nodes[id.0]
    }

    /// Returns one token by id.
    pub fn token(&self, id: TokenId) -> &PosToken {
        self.tokens
            .get(id.index())
            .expect("ICE: TokenId not present in SyntaxTree")
    }

    /// Returns a node's parent id.
    pub fn parent(&self, id: CstId) -> Option<CstId> {
        self.node(id).parent
    }

    /// Returns a token's parent node id.
    pub fn token_parent(&self, id: TokenId) -> Option<CstId> {
        self.token_parents.get(id.index()).copied().flatten()
    }

    /// Returns comments explicitly attached before this token.
    pub fn leading_comments(&self, id: TokenId) -> &[TokenId] {
        self.leading_comments
            .get(id.index())
            .expect("ICE: TokenId not present in SyntaxTree")
    }

    /// Returns comments explicitly attached after this token.
    pub fn trailing_comments(&self, id: TokenId) -> &[TokenId] {
        self.trailing_comments
            .get(id.index())
            .expect("ICE: TokenId not present in SyntaxTree")
    }

    /// Returns a node's ordered children.
    pub fn children(&self, id: CstId) -> &[NodeOrToken] {
        &self.node(id).children
    }

    /// Returns the deepest syntax node whose span contains the byte offset.
    pub fn node_at_offset(&self, offset: usize) -> Option<CstId> {
        if !span_contains_offset(self.node(self.root).span, offset) {
            return None;
        }

        Some(self.node_at_offset_from(self.root, offset))
    }

    /// Returns the non-synthetic token containing the byte offset.
    pub fn token_at_offset(&self, offset: usize) -> Option<TokenId> {
        self.tokens
            .iter()
            .enumerate()
            .find(|(_, token)| token_contains_offset(token, offset))
            .map(|(index, _)| TokenId::new(index))
    }

    /// Reconstructs source text by walking tokens through the syntax tree.
    pub fn reconstruct_text(&self, source: &str) -> String {
        let mut out = String::with_capacity(source.len());
        self.push_text(self.root, source, &mut out);
        out
    }

    fn push_node(&mut self, kind: SyntaxKind, parent: Option<CstId>) -> CstId {
        let id = self.nodes.alloc(SyntaxNode {
            kind,
            span: Span::new(0, 0),
            parent,
            children: Vec::new(),
            diagnostics: Vec::new(),
        });
        CstId(id)
    }

    fn node_mut(&mut self, id: CstId) -> &mut SyntaxNode {
        &mut self.nodes[id.0]
    }

    fn collapse_trivial_wrappers(&mut self) {
        rebuild::collapse_trivial_wrappers(self);
    }

    fn recompute_node_spans(&mut self) {
        self.recompute_node_span(self.root);
    }

    fn recompute_node_span(&mut self, id: CstId) -> Span {
        let children = self.node(id).children.clone();
        for child in children {
            if let NodeOrToken::Node(child) = child {
                self.recompute_node_span(child);
            }
        }

        let span = self.span_for_node(id);
        self.node_mut(id).span = span;
        span
    }

    fn assert_span_invariants(&self) {
        for (id, node) in self.nodes() {
            let parent_span = node.span();
            let mut previous = None;

            for child in node.children() {
                let Some(child_span) = self.child_span(child) else {
                    continue;
                };

                assert!(
                    span_contains_span(parent_span, child_span),
                    "ICE: CST parent {id:?} {parent_span:?} does not contain child span {child_span:?}"
                );

                if let Some(order_span) = self.child_order_span(child) {
                    if let Some(previous_span) = previous {
                        assert!(
                            spans_are_non_decreasing(previous_span, order_span),
                            "ICE: CST children of {id:?} are out of source order: {previous_span:?} before {order_span:?}"
                        );
                    }

                    previous = Some(order_span);
                }
            }
        }
    }

    fn insert_token_child(&mut self, parent: CstId, token: TokenId, before_child: Option<CstId>) {
        let children = &mut self.node_mut(parent).children;
        let child = NodeOrToken::Token(token);

        let Some(before_child) = before_child else {
            children.push(child);
            return;
        };

        let index = children
            .iter()
            .position(|existing| *existing == NodeOrToken::Node(before_child))
            .expect("ICE: CST trivia anchor must be a child of its placement parent");
        children.insert(index, child);
    }

    fn attach_comment_trivia(&mut self) {
        for (index, token) in self.tokens.iter().enumerate() {
            if !token_is_meaningful(token) {
                continue;
            }

            *self
                .leading_comments
                .get_mut(index)
                .expect("ICE: leading comment table must match token stream length") =
                leading_comments_before(&self.tokens, index);
            *self
                .trailing_comments
                .get_mut(index)
                .expect("ICE: trailing comment table must match token stream length") =
                trailing_comments_after(&self.tokens, index);
        }
    }

    fn span_for_node(&self, id: CstId) -> Span {
        let node = self.node(id);
        let mut span = None;

        for child in &node.children {
            let child_span = self.child_span(child);
            if let Some(child_span) = child_span {
                span = Some(merge_span(span, child_span));
            }
        }

        span = span.or_else(|| self.diagnostic_span(node));

        span.unwrap_or_else(|| Span::new(0, 0))
    }

    fn child_span(&self, child: &NodeOrToken) -> Option<Span> {
        match child {
            NodeOrToken::Node(id) => self.node_source_span(*id),
            NodeOrToken::Token(id) => Some(token_span(self.token(*id))),
        }
    }

    fn node_source_span(&self, id: CstId) -> Option<Span> {
        self.concrete_node_span(id)
            .or_else(|| self.diagnostic_span(self.node(id)))
    }

    fn diagnostic_span(&self, node: &SyntaxNode) -> Option<Span> {
        node.diagnostics.iter().fold(None, |span, diagnostic| {
            if let Some(anchor) = diagnostic.anchor_span()
                && anchor.file == self.file
            {
                Some(merge_span(span, anchor.span))
            } else {
                span
            }
        })
    }

    fn child_order_span(&self, child: &NodeOrToken) -> Option<Span> {
        match child {
            NodeOrToken::Node(id) => self.concrete_node_span(*id),
            NodeOrToken::Token(id) => Some(token_span(self.token(*id))),
        }
    }

    fn concrete_node_span(&self, id: CstId) -> Option<Span> {
        let mut span = None;

        for child in self.node(id).children() {
            let child_span = match child {
                NodeOrToken::Node(child) => self.concrete_node_span(*child),
                NodeOrToken::Token(token) => Some(token_span(self.token(*token))),
            };
            if let Some(child_span) = child_span {
                span = Some(merge_span(span, child_span));
            }
        }

        span
    }

    fn node_at_offset_from(&self, id: CstId, offset: usize) -> CstId {
        for child in &self.node(id).children {
            let NodeOrToken::Node(child) = child else {
                continue;
            };

            if span_contains_offset(self.node(*child).span, offset) {
                return self.node_at_offset_from(*child, offset);
            }
        }

        id
    }

    fn push_text(&self, id: CstId, source: &str, out: &mut String) {
        for child in &self.node(id).children {
            match child {
                NodeOrToken::Node(id) => self.push_text(*id, source, out),
                NodeOrToken::Token(id) => out.push_str(self.token(*id).source_text(source)),
            }
        }
    }
}

fn merge_span(current: Option<Span>, next: Span) -> Span {
    match current {
        Some(current) => Span::new(current.start.min(next.start), current.end.max(next.end)),
        None => next,
    }
}

fn token_span(token: &PosToken) -> Span {
    Span::from_usize_range(token.start()..token.end())
}

fn token_is_trivia(token: &PosToken) -> bool {
    token.kind.is_trivia()
}

fn token_is_meaningful(token: &PosToken) -> bool {
    !token.kind.is_trivia() && !token.is_synthetic() && !matches!(token.kind, Token::Eof)
}

fn token_is_comment(token: &PosToken) -> bool {
    matches!(token.kind, Token::LineComment | Token::BlockComment)
}

fn newline_has_blank_line(token: &PosToken) -> bool {
    token.end().saturating_sub(token.start()) > 1
}

fn mark_meaningful_tokens(stack: &[CstId], node_has_meaningful_token: &mut [bool]) {
    for id in stack {
        *node_has_meaningful_token
            .get_mut(id.index())
            .expect("ICE: CST node marker table must contain every open node") = true;
    }
}

fn trivia_placement(stack: &[CstId], node_has_meaningful_token: &[bool]) -> (CstId, Option<CstId>) {
    let top = *stack
        .last()
        .expect("ICE: CST trivia token event must be inside a syntax node");

    if node_has_meaningful_token_at(node_has_meaningful_token, top) {
        return (top, None);
    }

    // Leading trivia belongs before the current empty subtree, not inside its
    // deepest wrapper, so leaf spans start at their meaningful token.
    let mut empty_subtree_root = stack.len() - 1;
    while empty_subtree_root > 0
        && !node_has_meaningful_token_at(
            node_has_meaningful_token,
            stack_at(stack, empty_subtree_root - 1),
        )
    {
        empty_subtree_root -= 1;
    }

    if empty_subtree_root == 0 {
        (stack_at(stack, 0), None)
    } else {
        (
            stack_at(stack, empty_subtree_root - 1),
            Some(stack_at(stack, empty_subtree_root)),
        )
    }
}

fn node_has_meaningful_token_at(node_has_meaningful_token: &[bool], id: CstId) -> bool {
    *node_has_meaningful_token
        .get(id.index())
        .expect("ICE: CST node marker table must contain every open node")
}

fn stack_at(stack: &[CstId], index: usize) -> CstId {
    *stack
        .get(index)
        .expect("ICE: CST open-node stack index must be in bounds")
}

fn leading_comments_before(tokens: &[PosToken], anchor_index: usize) -> Vec<TokenId> {
    let mut comments = Vec::new();
    let mut index = anchor_index;

    while index > 0 {
        index -= 1;
        let token = token_at(tokens, index);

        match token.kind {
            Token::Whitespace | Token::VOpen | Token::VClose | Token::VSemi => {}
            Token::Newline => {
                if newline_has_blank_line(token) {
                    break;
                }
            }
            _ if token_is_comment(token) => comments.push(TokenId::new(index)),
            _ => break,
        }
    }

    comments.reverse();
    comments
}

fn trailing_comments_after(tokens: &[PosToken], anchor_index: usize) -> Vec<TokenId> {
    let anchor_line = token_at(tokens, anchor_index).line;
    let mut comments = Vec::new();

    for (index, token) in tokens.iter().enumerate().skip(anchor_index + 1) {
        match token.kind {
            Token::Whitespace if token.line == anchor_line => {}
            _ if token_is_comment(token) && token.line == anchor_line => {
                comments.push(TokenId::new(index));
                break;
            }
            _ => break,
        }
    }

    comments
}

fn token_at(tokens: &[PosToken], index: usize) -> &PosToken {
    tokens
        .get(index)
        .expect("ICE: CST comment scan token index must be in bounds")
}

fn span_contains_offset(span: Span, offset: usize) -> bool {
    let start = span.start as usize;
    let end = span.end as usize;

    if start == end {
        offset == start
    } else {
        start <= offset && offset < end
    }
}

fn span_contains_span(parent: Span, child: Span) -> bool {
    parent.start <= child.start && child.end <= parent.end
}

fn spans_are_non_decreasing(previous: Span, next: Span) -> bool {
    previous.start < next.start || (previous.start == next.start && previous.end <= next.end)
}

fn token_contains_offset(token: &PosToken, offset: usize) -> bool {
    if token.is_synthetic() {
        return false;
    }

    if token.start() == token.end() {
        matches!(token.kind, Token::Eof) && offset == token.start()
    } else {
        token.start() <= offset && offset < token.end()
    }
}

#[cfg(test)]
mod tests;
