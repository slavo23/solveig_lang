//! Token emission, CST boundaries, and canonical separators for formatter documents.

use pretty::BoxDoc;
use solveig_syntax::{
    cst::{CstId, NodeOrToken, SyntaxTree, TokenId},
    lexer::Token,
    syntax::SyntaxKind,
};

use super::{BoundedDocument, Document, node};

pub(super) fn joined_children<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    parent: CstId,
    children: &[NodeOrToken],
    width: usize,
) -> BoundedDocument<'a> {
    let parent_kind = tree.node(parent).kind();
    let mut document = BoxDoc::nil();
    let mut first_boundary = None;
    let mut previous = None;
    let mut previous_element = None;

    for child in children {
        let Some((next, first, last)) = element(tree, source, *child, width) else {
            continue;
        };
        if let (Some(previous), Some(first)) = (previous, first) {
            let join = if parent_kind == SyntaxKind::AppExpr
                && tree.token(previous).kind == Token::RParen
                && previous_element.is_some_and(|element| contains_layout(tree, element))
            {
                BoxDoc::space()
            } else {
                separator(tree, parent_kind, previous, first)
            };
            document = document.append(join);
        }
        document = document.append(next);
        first_boundary = first_boundary.or(first);
        previous = last.or(previous);
        previous_element = Some(*child);
    }

    (document, first_boundary, previous)
}

pub(super) fn element<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    child: NodeOrToken,
    width: usize,
) -> Option<BoundedDocument<'a>> {
    match child {
        NodeOrToken::Node(id) => {
            let document = node(tree, source, id, width);
            document.1.is_some().then_some(document)
        }
        NodeOrToken::Token(id) => token_document(tree, source, id),
    }
}

fn token_document<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: TokenId,
) -> Option<BoundedDocument<'a>> {
    let token = tree.token(id);
    if token.is_synthetic() || matches!(token.kind, Token::Whitespace | Token::Newline | Token::Eof)
    {
        return None;
    }

    let document = if matches!(token.kind, Token::LineComment | Token::BlockComment) {
        comment_document(token.source_text(source))
    } else {
        BoxDoc::text(token.source_text(source))
    };

    Some((document, Some(id), Some(id)))
}

pub(super) fn separator<'a>(
    tree: &'a SyntaxTree,
    parent: SyntaxKind,
    previous: TokenId,
    next: TokenId,
) -> Document<'a> {
    let previous_token = tree.token(previous);
    let next_token = tree.token(next);

    if matches!(previous_token.kind, Token::LineComment) {
        return BoxDoc::hardline();
    }
    if matches!(previous_token.kind, Token::BlockComment) && next_token.line > previous_token.line {
        return BoxDoc::hardline();
    }
    if matches!(next_token.kind, Token::LineComment | Token::BlockComment) {
        return if next_token.line == previous_token.line {
            BoxDoc::space()
        } else {
            BoxDoc::hardline()
        };
    }

    match (previous_token.kind, next_token.kind) {
        (_, Token::RParen | Token::RBrack | Token::Comma) | (Token::LParen | Token::LBrack, _) => {
            BoxDoc::nil()
        }
        (_, Token::Backtick) | (Token::Backtick, _) if parent == SyntaxKind::BacktickOperator => {
            BoxDoc::nil()
        }
        (_, Token::Backtick) if parent == SyntaxKind::OpChain => BoxDoc::space(),
        (Token::Backtick | Token::Dot, _) if parent == SyntaxKind::OpChain => BoxDoc::line(),
        (Token::Comma, _) | (_, Token::Pipe) | (_, Token::In | Token::Then | Token::Else) => {
            BoxDoc::line()
        }
        (Token::Semicolon, _) => BoxDoc::hardline(),
        (
            Token::Eq
            | Token::Colon
            | Token::Arrow
            | Token::BindArrow
            | Token::FatArrow
            | Token::Operator
            | Token::ReservedOp
            | Token::Cons,
            _,
        ) => BoxDoc::line(),
        (_, Token::Dot) if matches!(parent, SyntaxKind::NamePath | SyntaxKind::ImportDecl) => {
            BoxDoc::nil()
        }
        (Token::Dot, _) if matches!(parent, SyntaxKind::NamePath | SyntaxKind::ImportDecl) => {
            BoxDoc::nil()
        }
        (_, Token::Dot) if parent == SyntaxKind::Type => BoxDoc::nil(),
        (Token::Dot, _) if parent == SyntaxKind::Type => BoxDoc::space(),
        (_, Token::LParen) if parent == SyntaxKind::ImportDecl => BoxDoc::nil(),
        (Token::Minus, _) if matches!(parent, SyntaxKind::OpOperand | SyntaxKind::PatternArg) => {
            BoxDoc::nil()
        }
        _ if breakable_application(parent) => BoxDoc::line(),
        _ => BoxDoc::space(),
    }
}

pub(super) fn block_separator<'a>(
    tree: &'a SyntaxTree,
    parent: SyntaxKind,
    previous: TokenId,
    next: TokenId,
) -> Document<'a> {
    let previous_token = tree.token(previous);
    let next_token = tree.token(next);
    let lines = next_token.line.saturating_sub(previous_token.line);

    if lines > 1 {
        BoxDoc::hardline().append(BoxDoc::hardline())
    } else if lines == 1
        || (matches!(parent, SyntaxKind::Module | SyntaxKind::DefinitionList)
            && !matches!(next_token.kind, Token::LineComment | Token::BlockComment))
    {
        BoxDoc::hardline()
    } else {
        separator(tree, parent, previous, next)
    }
}

pub(super) fn breakable_application(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AppExpr | SyntaxKind::TypeApp | SyntaxKind::PatternHead | SyntaxKind::Predicate
    )
}

pub(super) fn generic_nesting(tree: &SyntaxTree, id: CstId) -> usize {
    let mut nesting = 0;
    let mut current = tree.parent(id);
    while let Some(node) = current {
        if matches!(
            tree.node(node).kind(),
            SyntaxKind::FunctionDef
                | SyntaxKind::DoStmt
                | SyntaxKind::CaseArm
                | SyntaxKind::AppExpr
                | SyntaxKind::LambdaExpr
                | SyntaxKind::LetExpr
                | SyntaxKind::IfExpr
                | SyntaxKind::OpChain
        ) {
            nesting += 1;
        }
        current = tree.parent(node);
    }
    nesting
}

pub(super) fn node_boundaries(tree: &SyntaxTree, id: CstId) -> (Option<TokenId>, Option<TokenId>) {
    let first = tree
        .children(id)
        .iter()
        .find_map(|child| first_boundary(tree, *child));
    let last = tree
        .children(id)
        .iter()
        .rev()
        .find_map(|child| last_boundary(tree, *child));
    (first, last)
}

pub(super) fn first_boundary(tree: &SyntaxTree, child: NodeOrToken) -> Option<TokenId> {
    match child {
        NodeOrToken::Node(id) => tree
            .children(id)
            .iter()
            .find_map(|child| first_boundary(tree, *child)),
        NodeOrToken::Token(id) => is_emitted_token(tree, id).then_some(id),
    }
}

pub(super) fn last_boundary(tree: &SyntaxTree, child: NodeOrToken) -> Option<TokenId> {
    match child {
        NodeOrToken::Node(id) => tree
            .children(id)
            .iter()
            .rev()
            .find_map(|child| last_boundary(tree, *child)),
        NodeOrToken::Token(id) => is_emitted_token(tree, id).then_some(id),
    }
}

pub(super) fn contains_layout(tree: &SyntaxTree, child: NodeOrToken) -> bool {
    match child {
        NodeOrToken::Node(id) => {
            matches!(
                tree.node(id).kind(),
                SyntaxKind::CaseExpr | SyntaxKind::DoExpr
            ) || tree
                .children(id)
                .iter()
                .any(|child| contains_layout(tree, *child))
        }
        NodeOrToken::Token(_) => false,
    }
}

pub(super) fn starts_multiline_expression(tree: &SyntaxTree, child: NodeOrToken) -> bool {
    match child {
        NodeOrToken::Node(id) => {
            if matches!(
                tree.node(id).kind(),
                SyntaxKind::LambdaExpr
                    | SyntaxKind::LetExpr
                    | SyntaxKind::IfExpr
                    | SyntaxKind::CaseExpr
                    | SyntaxKind::DoExpr
            ) {
                true
            } else if tree.node(id).kind() == SyntaxKind::Expr {
                tree.children(id)
                    .iter()
                    .any(|child| starts_multiline_expression(tree, *child))
            } else {
                false
            }
        }
        NodeOrToken::Token(_) => false,
    }
}

fn is_emitted_token(tree: &SyntaxTree, id: TokenId) -> bool {
    let token = tree.token(id);
    !token.is_synthetic() && !matches!(token.kind, Token::Whitespace | Token::Newline | Token::Eof)
}

pub(super) fn token_position(
    tree: &SyntaxTree,
    children: &[NodeOrToken],
    kind: Token,
) -> Option<usize> {
    token_position_after(tree, children, kind, 0)
}

pub(super) fn token_position_after(
    tree: &SyntaxTree,
    children: &[NodeOrToken],
    kind: Token,
    start: usize,
) -> Option<usize> {
    children
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, child)| match child {
            NodeOrToken::Token(id) if tree.token(*id).kind == kind => Some(index),
            _ => None,
        })
}

pub(super) fn last_header_element(
    tree: &SyntaxTree,
    children: &[NodeOrToken],
    end: usize,
) -> usize {
    children
        .get(..end)
        .unwrap_or(&[])
        .iter()
        .rposition(|child| match child {
            NodeOrToken::Node(_) => true,
            NodeOrToken::Token(id) => {
                let token = tree.token(*id);
                !token.is_synthetic()
                    && !matches!(
                        token.kind,
                        Token::Whitespace
                            | Token::Newline
                            | Token::LineComment
                            | Token::BlockComment
                    )
            }
        })
        .map_or(0, |index| index + 1)
}

pub(super) fn is_layout_separator(tree: &SyntaxTree, child: NodeOrToken) -> bool {
    matches!(
        child,
        NodeOrToken::Token(id)
            if matches!(tree.token(id).kind, Token::VSemi | Token::Semicolon | Token::VOpen)
    )
}

pub(super) fn first_emitted_child(tree: &SyntaxTree, children: &[NodeOrToken]) -> Option<usize> {
    children
        .iter()
        .position(|child| first_boundary(tree, *child).is_some())
}

pub(super) fn last_emitted_child(tree: &SyntaxTree, children: &[NodeOrToken]) -> Option<usize> {
    children
        .iter()
        .rposition(|child| last_boundary(tree, *child).is_some())
}

pub(super) fn last_emitted_in(tree: &SyntaxTree, children: &[NodeOrToken]) -> Option<TokenId> {
    children
        .iter()
        .rev()
        .find_map(|child| last_boundary(tree, *child))
}

fn comment_document(text: &str) -> Document<'_> {
    let mut blank = false;
    let lines = text.lines().filter(|line| {
        let is_blank = line.trim().is_empty();
        let keep = !is_blank || !blank;
        blank = is_blank;
        keep
    });
    BoxDoc::intersperse(lines.map(BoxDoc::text), BoxDoc::hardline())
}

pub(super) fn absolute_indent(document: Document<'_>, column: u32) -> Document<'_> {
    let column = isize::try_from(column)
        .expect("ICE: source indentation must fit the formatter nesting type");
    document.nest(column).nest(-isize::MAX)
}

pub(super) fn push_nonempty_part<'a>(
    parts: &mut Vec<Document<'a>>,
    document: BoundedDocument<'a>,
    indent: isize,
) {
    if document.1.is_some() {
        parts.push(document.0.nest(indent).group());
    }
}

pub(super) fn slice(children: &[NodeOrToken], start: usize, end: usize) -> &[NodeOrToken] {
    children.get(start..end).unwrap_or(&[])
}

pub(super) fn slice_to(children: &[NodeOrToken], end: usize) -> &[NodeOrToken] {
    children.get(..end).unwrap_or(&[])
}
