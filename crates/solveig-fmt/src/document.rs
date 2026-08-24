//! CST-to-document construction.

use pretty::BoxDoc;
use solveig_syntax::{
    cst::{CstId, NodeOrToken, SyntaxTree, TokenId},
    lexer::Token,
    syntax::SyntaxKind,
};

mod tokens;

use tokens::*;

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
        SyntaxKind::LetExpr => let_expression(tree, source, id, width),
        SyntaxKind::ParenExpr
        | SyntaxKind::TupleExpr
        | SyntaxKind::TuplePattern
        | SyntaxKind::TupleType
        | SyntaxKind::ListExpr
        | SyntaxKind::OperatorSection
        | SyntaxKind::PredicateList
        | SyntaxKind::PatternArg
        | SyntaxKind::TypeAtom => delimited(tree, source, id, width),
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
        || matches!(
            kind,
            SyntaxKind::Type | SyntaxKind::QualifiedType | SyntaxKind::TypeArrow
        )
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
    let children = tree.children(id);
    let last_import = if matches!(kind, SyntaxKind::Module | SyntaxKind::ImportList) {
        children.iter().rposition(|child| {
            matches!(
                child,
                NodeOrToken::Node(child)
                    if matches!(
                        tree.node(*child).kind(),
                        SyntaxKind::ImportList | SyntaxKind::ImportDecl
                    )
            )
        })
    } else {
        None
    };
    let mut document = BoxDoc::nil();
    let mut previous = None;
    let mut previous_child = None;
    let mut separated_imports = false;

    for (position, child) in children.iter().enumerate() {
        let Some((next, first, last)) = element(tree, source, *child, width) else {
            continue;
        };

        if let (Some(previous), Some(first)) = (previous, first) {
            let separates_imports = !separated_imports
                && last_import.is_some_and(|last_import| position > last_import)
                && !(matches!(
                    tree.token(first).kind,
                    Token::LineComment | Token::BlockComment
                ) && tree.token(first).line == tree.token(previous).line);
            let trivia_already_separated = kind == SyntaxKind::Module
                && previous_child.is_some_and(|child| match child {
                    NodeOrToken::Node(child) => {
                        tree.node(child).kind() == SyntaxKind::ImportList
                            && has_detached_trailing_import_trivia(tree, child)
                    }
                    NodeOrToken::Token(_) => false,
                });
            let separator = if separates_imports && !trivia_already_separated {
                BoxDoc::hardline().append(BoxDoc::hardline())
            } else {
                block_separator(tree, kind, previous, first)
            };
            document = document.append(separator);
            separated_imports |= separates_imports;
        }
        document = document.append(next);
        previous = last.or(previous);
        previous_child = Some(*child);
    }

    document
}

fn has_detached_trailing_import_trivia(tree: &SyntaxTree, id: CstId) -> bool {
    let children = tree.children(id);
    let Some(last_import) = children.iter().rposition(|child| {
        matches!(
            child,
            NodeOrToken::Node(child) if tree.node(*child).kind() == SyntaxKind::ImportDecl
        )
    }) else {
        return false;
    };
    let Some(import_child) = children.get(last_import) else {
        return false;
    };
    let Some(import_end) = last_boundary(tree, *import_child) else {
        return false;
    };
    let Some(trivia_start) = children
        .get(last_import + 1..)
        .unwrap_or(&[])
        .iter()
        .find_map(|child| first_boundary(tree, *child))
    else {
        return false;
    };

    tree.token(trivia_start).line > tree.token(import_end).line
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

fn let_expression<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> Document<'a> {
    let children = tree.children(id);
    let Some(equals) = token_position(tree, children, Token::Eq) else {
        return joined_node(tree, source, id, width);
    };
    let Some(in_keyword) = token_position_after(tree, children, Token::In, equals + 1) else {
        return joined_node(tree, source, id, width);
    };

    let header = joined_children(tree, source, id, slice_to(children, equals + 1), width).0;
    let initializer = joined_children(
        tree,
        source,
        id,
        slice(children, equals + 1, in_keyword),
        width,
    )
    .0;
    let continuation = joined_children(
        tree,
        source,
        id,
        children.get(in_keyword..).unwrap_or(&[]),
        width,
    )
    .0;

    header
        .append(BoxDoc::line().append(initializer).nest(INDENT))
        .append(BoxDoc::line())
        .append(continuation)
        .group()
}

fn delimited<'a>(tree: &'a SyntaxTree, source: &'a str, id: CstId, width: usize) -> Document<'a> {
    let children = tree.children(id);
    let first = first_emitted_child(tree, children);
    let last = last_emitted_child(tree, children);

    match (first, last) {
        (Some(first), Some(last)) if first < last => {
            let Some((first_child, last_child)) = children.get(first).zip(children.get(last))
            else {
                return joined_children(tree, source, id, children, width).0.group();
            };
            if !delimiters_match(tree, *first_child, *last_child) {
                return joined_children(tree, source, id, children, width).0.group();
            }
            let open =
                joined_children(tree, source, id, slice(children, first, first + 1), width).0;
            let body = joined_children(tree, source, id, slice(children, first + 1, last), width).0;
            let close = joined_children(tree, source, id, slice(children, last, last + 1), width).0;
            if matches!(
                tree.node(id).kind(),
                SyntaxKind::TupleExpr
                    | SyntaxKind::TuplePattern
                    | SyntaxKind::TupleType
                    | SyntaxKind::ListExpr
            ) {
                open.append(BoxDoc::line_().append(body).nest(INDENT))
                    .append(BoxDoc::line_())
                    .append(close)
                    .group()
            } else if tree.node(id).kind() == SyntaxKind::ParenExpr
                && children
                    .get(first + 1..last)
                    .unwrap_or(&[])
                    .iter()
                    .any(|child| starts_multiline_expression(tree, *child))
            {
                open.append(body)
                    .append(BoxDoc::line_())
                    .append(close)
                    .group()
            } else {
                open.append(body).append(close).group()
            }
        }
        _ => joined_children(tree, source, id, children, width).0.group(),
    }
}

fn delimiters_match(tree: &SyntaxTree, first: NodeOrToken, last: NodeOrToken) -> bool {
    match (first, last) {
        (NodeOrToken::Token(first), NodeOrToken::Token(last)) => matches!(
            (tree.token(first).kind, tree.token(last).kind),
            (Token::LParen, Token::RParen) | (Token::LBrack, Token::RBrack)
        ),
        _ => false,
    }
}
