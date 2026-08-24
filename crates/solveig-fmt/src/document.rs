//! CST-to-document construction.

use pretty::BoxDoc;
use solveig_syntax::{
    cst::{CstId, NodeOrToken, SyntaxTree, TokenId},
    lexer::Token,
    syntax::SyntaxKind,
};

const INDENT: isize = 4;

type Document<'a> = BoxDoc<'a>;
type BoundedDocument<'a> = (Document<'a>, Option<TokenId>, Option<TokenId>);

pub(super) fn root<'a>(tree: &'a SyntaxTree, source: &'a str) -> Document<'a> {
    node(tree, source, tree.root()).0
}

fn node<'a>(tree: &'a SyntaxTree, source: &'a str, id: CstId) -> BoundedDocument<'a> {
    let kind = tree.node(id).kind();
    let document = match kind {
        SyntaxKind::Module | SyntaxKind::ImportList | SyntaxKind::DefinitionList => {
            block(tree, source, id)
        }
        SyntaxKind::ClassDef | SyntaxKind::InstanceDef | SyntaxKind::DoExpr => {
            layout_block(tree, source, id)
        }
        SyntaxKind::CaseExpr => case_expression(tree, source, id),
        SyntaxKind::ParenExpr
        | SyntaxKind::TupleExpr
        | SyntaxKind::TuplePattern
        | SyntaxKind::TupleType
        | SyntaxKind::ListExpr
        | SyntaxKind::OperatorSection => delimited(tree, source, id),
        _ => joined_children(tree, source, id, tree.children(id))
            .0
            .nest(INDENT)
            .group(),
    };
    let (first, last) = node_boundaries(tree, id);
    (document, first, last)
}

fn block<'a>(tree: &'a SyntaxTree, source: &'a str, id: CstId) -> Document<'a> {
    let kind = tree.node(id).kind();
    let mut document = BoxDoc::nil();
    let mut previous = None;

    for child in tree.children(id) {
        let Some((next, first, last)) = element(tree, source, *child) else {
            continue;
        };

        if let (Some(previous), Some(first)) = (previous, first) {
            document = document.append(block_separator(tree, kind, previous, first));
        }
        document = document.append(next);
        previous = last.or(previous);
    }

    document
}

fn layout_block<'a>(tree: &'a SyntaxTree, source: &'a str, id: CstId) -> Document<'a> {
    let children = tree.children(id);
    let Some(open) = token_position(tree, children, Token::VOpen) else {
        return joined_children(tree, source, id, children)
            .0
            .nest(INDENT)
            .group();
    };
    let close =
        token_position_after(tree, children, Token::VClose, open + 1).unwrap_or(children.len());
    let header_end = last_header_element(tree, children, open);
    let header = joined_children(tree, source, id, slice_to(children, header_end)).0;

    let mut body_parts = Vec::new();
    let mut start = header_end;
    for (offset, child) in children
        .get(header_end..close)
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        if is_layout_separator(tree, *child) {
            push_nonempty_part(
                &mut body_parts,
                joined_children(
                    tree,
                    source,
                    id,
                    slice(children, start, header_end + offset),
                ),
            );
            start = header_end + offset + 1;
        }
    }
    push_nonempty_part(
        &mut body_parts,
        joined_children(tree, source, id, slice(children, start, close)),
    );

    if body_parts.is_empty() {
        header.group()
    } else {
        header.append(
            BoxDoc::hardline()
                .append(BoxDoc::intersperse(body_parts, BoxDoc::hardline()))
                .nest(INDENT),
        )
    }
}

fn case_expression<'a>(tree: &'a SyntaxTree, source: &'a str, id: CstId) -> Document<'a> {
    let children = tree.children(id);
    let first_arm = children.iter().position(|child| {
        matches!(
            child,
            NodeOrToken::Node(child) if tree.node(*child).kind() == SyntaxKind::CaseArm
        )
    });
    let Some(first_arm) = first_arm else {
        return joined_children(tree, source, id, children)
            .0
            .nest(INDENT)
            .group();
    };

    let header_end = last_header_element(tree, children, first_arm);
    let header = joined_children(tree, source, id, slice_to(children, header_end)).0;
    let leading = joined_children(tree, source, id, slice(children, header_end, first_arm));
    let mut arms = Vec::new();
    if leading.1.is_some() {
        arms.push(leading.0);
    }
    arms.extend(
        children
            .get(first_arm..)
            .unwrap_or(&[])
            .iter()
            .filter_map(|child| match child {
                NodeOrToken::Node(child) if tree.node(*child).kind() == SyntaxKind::CaseArm => {
                    Some(node(tree, source, *child).0)
                }
                _ => None,
            }),
    );

    header.group().append(
        BoxDoc::hardline()
            .append(BoxDoc::intersperse(arms, BoxDoc::hardline()))
            .nest(INDENT),
    )
}

fn delimited<'a>(tree: &'a SyntaxTree, source: &'a str, id: CstId) -> Document<'a> {
    let children = tree.children(id);
    let first = first_emitted_child(tree, children);
    let last = last_emitted_child(tree, children);

    match (first, last) {
        (Some(first), Some(last)) if first < last => {
            let open = joined_children(tree, source, id, slice(children, first, first + 1)).0;
            let body = joined_children(tree, source, id, slice(children, first + 1, last)).0;
            let close = joined_children(tree, source, id, slice(children, last, last + 1)).0;
            open.append(body.nest(INDENT)).append(close).group()
        }
        _ => joined_children(tree, source, id, children).0.group(),
    }
}

fn joined_children<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    parent: CstId,
    children: &[NodeOrToken],
) -> BoundedDocument<'a> {
    let parent_kind = tree.node(parent).kind();
    let mut document = BoxDoc::nil();
    let mut first_boundary = None;
    let mut previous = None;

    for child in children {
        let Some((next, first, last)) = element(tree, source, *child) else {
            continue;
        };
        if let (Some(previous), Some(first)) = (previous, first) {
            document = document.append(separator(tree, parent_kind, previous, first));
        }
        document = document.append(next);
        first_boundary = first_boundary.or(first);
        previous = last.or(previous);
    }

    (document, first_boundary, previous)
}

fn element<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    child: NodeOrToken,
) -> Option<BoundedDocument<'a>> {
    match child {
        NodeOrToken::Node(id) => {
            let document = node(tree, source, id);
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

fn separator<'a>(
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
        (Token::Backtick, _) if parent == SyntaxKind::OpChain => BoxDoc::line(),
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

fn block_separator<'a>(
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
    } else if lines == 1 || matches!(parent, SyntaxKind::Module | SyntaxKind::DefinitionList) {
        BoxDoc::hardline()
    } else {
        separator(tree, parent, previous, next)
    }
}

fn breakable_application(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AppExpr | SyntaxKind::TypeApp | SyntaxKind::PatternHead | SyntaxKind::Predicate
    )
}

fn node_boundaries(tree: &SyntaxTree, id: CstId) -> (Option<TokenId>, Option<TokenId>) {
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

fn first_boundary(tree: &SyntaxTree, child: NodeOrToken) -> Option<TokenId> {
    match child {
        NodeOrToken::Node(id) => tree
            .children(id)
            .iter()
            .find_map(|child| first_boundary(tree, *child)),
        NodeOrToken::Token(id) => is_emitted_token(tree, id).then_some(id),
    }
}

fn last_boundary(tree: &SyntaxTree, child: NodeOrToken) -> Option<TokenId> {
    match child {
        NodeOrToken::Node(id) => tree
            .children(id)
            .iter()
            .rev()
            .find_map(|child| last_boundary(tree, *child)),
        NodeOrToken::Token(id) => is_emitted_token(tree, id).then_some(id),
    }
}

fn is_emitted_token(tree: &SyntaxTree, id: TokenId) -> bool {
    let token = tree.token(id);
    !token.is_synthetic() && !matches!(token.kind, Token::Whitespace | Token::Newline | Token::Eof)
}

fn token_position(tree: &SyntaxTree, children: &[NodeOrToken], kind: Token) -> Option<usize> {
    token_position_after(tree, children, kind, 0)
}

fn token_position_after(
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

fn last_header_element(tree: &SyntaxTree, children: &[NodeOrToken], end: usize) -> usize {
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

fn is_layout_separator(tree: &SyntaxTree, child: NodeOrToken) -> bool {
    matches!(
        child,
        NodeOrToken::Token(id)
            if matches!(tree.token(id).kind, Token::VSemi | Token::Semicolon | Token::VOpen)
    )
}

fn first_emitted_child(tree: &SyntaxTree, children: &[NodeOrToken]) -> Option<usize> {
    children
        .iter()
        .position(|child| first_boundary(tree, *child).is_some())
}

fn last_emitted_child(tree: &SyntaxTree, children: &[NodeOrToken]) -> Option<usize> {
    children
        .iter()
        .rposition(|child| last_boundary(tree, *child).is_some())
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

fn push_nonempty_part<'a>(parts: &mut Vec<Document<'a>>, document: BoundedDocument<'a>) {
    if document.1.is_some() {
        parts.push(document.0.group());
    }
}

fn slice(children: &[NodeOrToken], start: usize, end: usize) -> &[NodeOrToken] {
    children.get(start..end).unwrap_or(&[])
}

fn slice_to(children: &[NodeOrToken], end: usize) -> &[NodeOrToken] {
    children.get(..end).unwrap_or(&[])
}
