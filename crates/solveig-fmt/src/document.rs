//! CST-to-document construction.

use pretty::BoxDoc;
use solveig_syntax::{
    cst::{CstId, NodeOrToken, SyntaxTree, TokenId},
    lexer::Token,
    syntax::SyntaxKind,
};

const INDENT: isize = 4;
const INDENT_WIDTH: usize = 4;

pub(super) type Document<'a> = BoxDoc<'a>;
pub(super) type BoundedDocument<'a> = (Document<'a>, Option<TokenId>, Option<TokenId>);

pub(super) fn root<'a>(tree: &'a SyntaxTree, source: &'a str, width: usize) -> Document<'a> {
    node(tree, source, tree.root(), width).0
}

pub(super) fn node<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> BoundedDocument<'a> {
    let kind = tree.node(id).kind();
    let document = match kind {
        SyntaxKind::Module | SyntaxKind::ImportList | SyntaxKind::DefinitionList => {
            block(tree, source, id, width)
        }
        SyntaxKind::ClassDef | SyntaxKind::InstanceDef | SyntaxKind::DoExpr => {
            layout_block(tree, source, id, width)
        }
        SyntaxKind::FunctionDef | SyntaxKind::DoStmt => {
            layout_expression_parent(tree, source, id, width)
                .unwrap_or_else(|| joined_node(tree, source, id, width))
        }
        SyntaxKind::CaseExpr => case_expression(tree, source, id, width),
        SyntaxKind::ParenExpr
        | SyntaxKind::TupleExpr
        | SyntaxKind::TuplePattern
        | SyntaxKind::TupleType
        | SyntaxKind::ListExpr
        | SyntaxKind::OperatorSection => delimited(tree, source, id, width),
        _ => joined_node(tree, source, id, width),
    };
    let (first, last) = node_boundaries(tree, id);
    (document, first, last)
}

fn joined_node<'a>(tree: &'a SyntaxTree, source: &'a str, id: CstId, width: usize) -> Document<'a> {
    let kind = tree.node(id).kind();
    let flattened = tree.parent(id).is_some_and(|parent| {
        let parent = tree.node(parent).kind();
        kind == parent && (breakable_application(kind) || matches!(kind, SyntaxKind::LetExpr))
    });
    let layout_statement = matches!(kind, SyntaxKind::DoStmt | SyntaxKind::CaseArm);
    let maximum_generic_indent = (width / 4).max(INDENT_WIDTH * 2);
    let indent = if flattened
        || (!layout_statement && generic_nesting(tree, id) * INDENT_WIDTH >= maximum_generic_indent)
    {
        0
    } else {
        INDENT
    };
    joined_children(tree, source, id, tree.children(id), width)
        .0
        .nest(indent)
        .group()
}

fn layout_expression_parent<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> Option<Document<'a>> {
    let children = tree.children(id);
    let layout = children.iter().position(|child| {
        matches!(
            child,
            NodeOrToken::Node(child) if tree.node(*child).kind() == SyntaxKind::DoExpr
        )
    })?;
    let prefix = joined_children(tree, source, id, slice_to(children, layout), width);
    let body = match children.get(layout) {
        Some(NodeOrToken::Node(body)) => node(tree, source, *body, width),
        _ => return None,
    };
    let suffix = joined_children(
        tree,
        source,
        id,
        children.get(layout + 1..).unwrap_or(&[]),
        width,
    );

    let mut document = if let Some(previous) = prefix.2 {
        let join = if matches!(tree.token(previous).kind, Token::Eq | Token::BindArrow) {
            BoxDoc::space()
        } else {
            separator(tree, tree.node(id).kind(), previous, body.1?)
        };
        prefix.0.append(join).append(body.0)
    } else {
        body.0
    };
    if let (Some(previous), Some(next)) = (body.2, suffix.1) {
        document = document
            .append(separator(tree, tree.node(id).kind(), previous, next))
            .append(suffix.0);
    }
    Some(document)
}

fn block<'a>(tree: &'a SyntaxTree, source: &'a str, id: CstId, width: usize) -> Document<'a> {
    let kind = tree.node(id).kind();
    let mut document = BoxDoc::nil();
    let mut previous = None;

    for child in tree.children(id) {
        let Some((next, first, last)) = element(tree, source, *child, width) else {
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

fn layout_block<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> Document<'a> {
    let children = tree.children(id);
    let part_indent = matches!(
        tree.node(id).kind(),
        SyntaxKind::ClassDef | SyntaxKind::InstanceDef
    )
    .then_some(INDENT)
    .unwrap_or(0);
    let Some(open) = token_position(tree, children, Token::VOpen) else {
        return joined_children(tree, source, id, children, width)
            .0
            .nest(INDENT)
            .group();
    };
    let close =
        token_position_after(tree, children, Token::VClose, open + 1).unwrap_or(children.len());
    let body_indent = children
        .get(open + 1..close)
        .unwrap_or(&[])
        .iter()
        .find_map(|child| first_boundary(tree, *child))
        .map_or(0, |token| tree.token(token).col);
    let trailing_start = children
        .get(open + 1..close)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .find_map(|(offset, child)| match child {
            NodeOrToken::Token(token)
                if matches!(
                    tree.token(*token).kind,
                    Token::LineComment | Token::BlockComment
                ) && tree.token(*token).col < body_indent =>
            {
                Some(open + 1 + offset)
            }
            _ => None,
        });
    let body_end = trailing_start.unwrap_or(close);
    let header_end = last_header_element(tree, children, open);
    let header = joined_children(tree, source, id, slice_to(children, header_end), width).0;

    let mut body_parts = Vec::new();
    let mut start = header_end;
    for (offset, child) in children
        .get(header_end..body_end)
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
                    width,
                ),
                part_indent,
            );
            start = header_end + offset + 1;
        }
    }
    push_nonempty_part(
        &mut body_parts,
        joined_children(tree, source, id, slice(children, start, body_end), width),
        part_indent,
    );

    let header = header.nest(INDENT).group();
    let mut document = if body_parts.is_empty() {
        header
    } else {
        header.append(
            BoxDoc::hardline()
                .append(BoxDoc::intersperse(body_parts, BoxDoc::hardline()))
                .nest(INDENT),
        )
    };

    let trailing_start = trailing_start.unwrap_or_else(|| close.saturating_add(1));
    let trailing = joined_children(
        tree,
        source,
        id,
        children.get(trailing_start..).unwrap_or(&[]),
        width,
    );
    if let (Some(previous), Some(next)) = (
        last_emitted_in(tree, slice_to(children, body_end)),
        trailing.1,
    ) {
        document = document.append(absolute_indent(
            block_separator(tree, tree.node(id).kind(), previous, next).append(trailing.0),
            tree.token(next).col,
        ));
    }
    document
}

fn case_expression<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> Document<'a> {
    let children = tree.children(id);
    let close = token_position(tree, children, Token::VClose).unwrap_or(children.len());
    let arm_starts = children
        .iter()
        .enumerate()
        .take(close)
        .filter_map(|(index, child)| {
            matches!(
                child,
                NodeOrToken::Node(child) if tree.node(*child).kind() == SyntaxKind::CaseArm
            )
            .then_some(index)
        })
        .collect::<Vec<_>>();
    let Some(first_arm) = arm_starts.first().copied() else {
        return joined_children(tree, source, id, children, width)
            .0
            .nest(INDENT)
            .group();
    };
    let arm_indent = children
        .get(first_arm)
        .and_then(|child| first_boundary(tree, *child))
        .map_or(0, |token| tree.token(token).col);
    let trailing_start = arm_starts.last().copied().and_then(|last_arm| {
        children
            .iter()
            .enumerate()
            .take(close)
            .skip(last_arm + 1)
            .find_map(|(index, child)| match child {
                NodeOrToken::Token(token)
                    if matches!(
                        tree.token(*token).kind,
                        Token::LineComment | Token::BlockComment
                    ) && tree.token(*token).col < arm_indent =>
                {
                    Some(index)
                }
                _ => None,
            })
    });
    let body_end = trailing_start.unwrap_or(close);

    let header_end = last_header_element(tree, children, first_arm);
    let header = joined_children(tree, source, id, slice_to(children, header_end), width).0;
    let leading = joined_children(
        tree,
        source,
        id,
        slice(children, header_end, first_arm),
        width,
    );
    let mut arms = Vec::new();
    if leading.1.is_some() {
        arms.push(leading.0);
    }
    for (position, start) in arm_starts.iter().copied().enumerate() {
        let end = arm_starts.get(position + 1).copied().unwrap_or(body_end);
        push_nonempty_part(
            &mut arms,
            joined_children(tree, source, id, slice(children, start, end), width),
            0,
        );
    }

    let mut document = header.group().append(
        BoxDoc::hardline()
            .append(BoxDoc::intersperse(arms, BoxDoc::hardline()))
            .nest(INDENT),
    );
    let trailing_start = trailing_start.unwrap_or_else(|| close.saturating_add(1));
    let trailing = joined_children(
        tree,
        source,
        id,
        children.get(trailing_start..).unwrap_or(&[]),
        width,
    );
    if let (Some(previous), Some(next)) = (
        last_emitted_in(tree, slice_to(children, body_end)),
        trailing.1,
    ) {
        document = document.append(absolute_indent(
            block_separator(tree, SyntaxKind::CaseExpr, previous, next).append(trailing.0),
            tree.token(next).col,
        ));
    }
    document
}

fn delimited<'a>(tree: &'a SyntaxTree, source: &'a str, id: CstId, width: usize) -> Document<'a> {
    let children = tree.children(id);
    let indent = matches!(tree.node(id).kind(), SyntaxKind::ParenExpr)
        .then_some(0)
        .unwrap_or(INDENT);
    let first = first_emitted_child(tree, children);
    let last = last_emitted_child(tree, children);

    match (first, last) {
        (Some(first), Some(last)) if first < last => {
            let open =
                joined_children(tree, source, id, slice(children, first, first + 1), width).0;
            let body = joined_children(tree, source, id, slice(children, first + 1, last), width).0;
            let close = joined_children(tree, source, id, slice(children, last, last + 1), width).0;
            open.append(body.nest(indent)).append(close).group()
        }
        _ => joined_children(tree, source, id, children, width).0.group(),
    }
}

fn joined_children<'a>(
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

fn element<'a>(
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

fn generic_nesting(tree: &SyntaxTree, id: CstId) -> usize {
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

fn contains_layout(tree: &SyntaxTree, child: NodeOrToken) -> bool {
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

fn last_emitted_in(tree: &SyntaxTree, children: &[NodeOrToken]) -> Option<TokenId> {
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

fn absolute_indent(document: Document<'_>, column: u32) -> Document<'_> {
    let column = isize::try_from(column)
        .expect("ICE: source indentation must fit the formatter nesting type");
    document.nest(column).nest(-isize::MAX)
}

fn push_nonempty_part<'a>(
    parts: &mut Vec<Document<'a>>,
    document: BoundedDocument<'a>,
    indent: isize,
) {
    if document.1.is_some() {
        parts.push(document.0.nest(indent).group());
    }
}

fn slice(children: &[NodeOrToken], start: usize, end: usize) -> &[NodeOrToken] {
    children.get(start..end).unwrap_or(&[])
}

fn slice_to(children: &[NodeOrToken], end: usize) -> &[NodeOrToken] {
    children.get(..end).unwrap_or(&[])
}
