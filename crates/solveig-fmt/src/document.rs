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
const DEFINITION_EQUALS_WIDTH: usize = 2;

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
        SyntaxKind::Module
        | SyntaxKind::ModuleDirectiveList
        | SyntaxKind::ImportList
        | SyntaxKind::DefinitionList => block(tree, source, id, width),
        SyntaxKind::ClassDef | SyntaxKind::InstanceDef | SyntaxKind::DoExpr => {
            layout_block(tree, source, id, width)
        }
        SyntaxKind::FunctionDef => function_definition(tree, source, id, width)
            .or_else(|| layout_expression_parent(tree, source, id, width))
            .unwrap_or_else(|| joined_node(tree, source, id, width)),
        SyntaxKind::DoStmt => layout_expression_parent(tree, source, id, width)
            .or_else(|| binding_statement(tree, source, id, width))
            .unwrap_or_else(|| joined_node(tree, source, id, width)),
        SyntaxKind::LambdaExpr => lambda_expression(tree, source, id, width)
            .or_else(|| layout_expression_parent(tree, source, id, width))
            .unwrap_or_else(|| joined_node(tree, source, id, width)),
        SyntaxKind::CaseArm => layout_expression_parent(tree, source, id, width)
            .unwrap_or_else(|| joined_node(tree, source, id, width)),
        SyntaxKind::DataDef => data_declaration(tree, source, id, width)
            .unwrap_or_else(|| joined_node(tree, source, id, width)),
        SyntaxKind::OpChain => operator_chain(tree, source, id, width)
            .unwrap_or_else(|| joined_node(tree, source, id, width)),
        SyntaxKind::CaseExpr => case_expression(tree, source, id, width),
        SyntaxKind::LetExpr => let_expression(tree, source, id, width),
        SyntaxKind::IfExpr => if_expression(tree, source, id, width)
            .unwrap_or_else(|| joined_node(tree, source, id, width)),
        SyntaxKind::ExportDecl
        | SyntaxKind::PrivateDecl
        | SyntaxKind::ReexportDecl
        | SyntaxKind::ImportDecl => module_selection(tree, source, id, width)
            .unwrap_or_else(|| joined_node(tree, source, id, width)),
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
    let indent = if flattened
        || matches!(
            kind,
            SyntaxKind::Type | SyntaxKind::QualifiedType | SyntaxKind::TypeArrow
        ) {
        0
    } else {
        INDENT
    };
    joined_children(tree, source, id, tree.children(id), width)
        .0
        .nest(indent)
        .group()
}

fn module_selection<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> Option<Document<'a>> {
    let children = tree.children(id);
    let open = token_position(tree, children, Token::LParen)?;
    let close = children
        .iter()
        .enumerate()
        .skip(open + 1)
        .rfind(|(_, child)| {
            matches!(child, NodeOrToken::Token(token) if tree.token(*token).kind == Token::RParen)
        })
        .map(|(position, _)| position)?;
    let prefix = joined_children(tree, source, id, slice_to(children, open + 1), width).0;
    let body = joined_children(tree, source, id, slice(children, open + 1, close), width).0;
    let suffix = joined_children(
        tree,
        source,
        id,
        children.get(close..).unwrap_or(&[]),
        width,
    )
    .0;

    Some(
        prefix
            .append(BoxDoc::line_().append(body).nest(INDENT))
            .append(BoxDoc::line_())
            .append(suffix)
            .group(),
    )
}

fn function_definition<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> Option<Document<'a>> {
    let children = tree.children(id);
    if children.iter().any(|child| contains_comment(tree, *child)) {
        return None;
    }
    let equals = token_position(tree, children, Token::Eq)?;
    let left_children = children.get(..equals).unwrap_or(&[]);
    let inline_left = joined_children(tree, source, id, left_children, usize::MAX).0;
    let inline_left = render_unbounded(&inline_left);
    let available_width = width.saturating_sub(enclosing_declaration_indent(tree, id));
    let left = if inline_left.chars().count() + DEFINITION_EQUALS_WIDTH <= available_width {
        BoxDoc::text(inline_left)
    } else {
        let parts = declaration_head_parts(tree, source, id, left_children, width);
        BoxDoc::intersperse(parts, BoxDoc::hardline()).nest(INDENT)
    };
    let equals_document =
        joined_children(tree, source, id, slice(children, equals, equals + 1), width).0;
    let body = joined_children(
        tree,
        source,
        id,
        children.get(equals + 1..).unwrap_or(&[]),
        width,
    );
    body.1?;

    let header = left.append(BoxDoc::space()).append(equals_document);
    let starts_with_do = children
        .get(equals + 1..)
        .unwrap_or(&[])
        .iter()
        .any(|child| starts_with_do_expression(tree, *child));
    Some(if starts_with_do {
        header.append(BoxDoc::space()).append(body.0)
    } else {
        header
            .append(BoxDoc::line().append(body.0).nest(INDENT))
            .group()
    })
}

fn declaration_head_parts<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    parent: CstId,
    children: &[NodeOrToken],
    width: usize,
) -> Vec<Document<'a>> {
    let Some(first) = first_emitted_child(tree, children) else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    let mut position = first;
    if matches!(
        children.get(first),
        Some(NodeOrToken::Token(token)) if tree.token(*token).kind == Token::LParen
    ) && let Some(close) =
        children
            .iter()
            .enumerate()
            .skip(first + 1)
            .find_map(|(index, child)| {
                matches!(
                    child,
                    NodeOrToken::Token(token) if tree.token(*token).kind == Token::RParen
                )
                .then_some(index)
            })
    {
        parts.push(
            joined_children(
                tree,
                source,
                parent,
                slice(children, first, close + 1),
                width,
            )
            .0,
        );
        position = close + 1;
    }
    if parts.is_empty() {
        if let Some(document) = children
            .get(first)
            .and_then(|child| element(tree, source, *child, width))
        {
            parts.push(document.0);
        }
        position = first + 1;
    }
    parts.extend(
        children
            .get(position..)
            .unwrap_or(&[])
            .iter()
            .filter_map(|child| element(tree, source, *child, width).map(|document| document.0)),
    );
    parts
}

fn render_unbounded(document: &Document<'_>) -> String {
    const MEASUREMENT_WIDTH: usize = 1_000_000;
    let mut output = Vec::new();
    document
        .render(MEASUREMENT_WIDTH, &mut output)
        .expect("ICE: rendering an in-memory formatter fragment must succeed");
    String::from_utf8(output).expect("ICE: formatter fragments must contain valid UTF-8")
}

fn enclosing_declaration_indent(tree: &SyntaxTree, id: CstId) -> usize {
    let mut indent = 0;
    let mut parent = tree.parent(id);
    while let Some(node) = parent {
        if matches!(
            tree.node(node).kind(),
            SyntaxKind::ClassDef | SyntaxKind::InstanceDef
        ) {
            indent += usize::try_from(INDENT).expect("ICE: formatter indentation is positive");
        }
        parent = tree.parent(node);
    }
    indent
}

fn binding_statement<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> Option<Document<'a>> {
    let children = tree.children(id);
    if children.iter().any(|child| contains_comment(tree, *child)) {
        return None;
    }
    let equals = token_position(tree, children, Token::Eq)?;
    let header = joined_children(tree, source, id, slice_to(children, equals + 1), width).0;
    let body = joined_children(
        tree,
        source,
        id,
        children.get(equals + 1..).unwrap_or(&[]),
        width,
    );
    body.1?;

    Some(
        header
            .append(BoxDoc::line().append(body.0).nest(INDENT))
            .group(),
    )
}

fn lambda_expression<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> Option<Document<'a>> {
    let children = tree.children(id);
    if children.iter().any(|child| contains_comment(tree, *child)) {
        return None;
    }
    let arrow = token_position(tree, children, Token::FatArrow)?;
    let mut parameters = children
        .get(..arrow)
        .unwrap_or(&[])
        .iter()
        .filter_map(|child| element(tree, source, *child, width).map(|document| document.0));
    let mut left = parameters.next()?;
    if let Some(first_parameter) = parameters.next() {
        left = left.append(BoxDoc::space()).append(first_parameter);
    }
    for parameter in parameters {
        left = left.append(BoxDoc::line().append(parameter).nest(INDENT));
    }
    let left = left.group();
    let arrow_document =
        joined_children(tree, source, id, slice(children, arrow, arrow + 1), width).0;
    let body_children = children.get(arrow + 1..).unwrap_or(&[]);
    let body = joined_children(tree, source, id, body_children, width);
    body.1?;
    let header = left.append(BoxDoc::space()).append(arrow_document);

    Some(
        if body_children
            .iter()
            .any(|child| starts_with_do_expression(tree, *child))
        {
            header.append(BoxDoc::space()).append(body.0)
        } else {
            header
                .append(BoxDoc::line().append(body.0).nest(INDENT))
                .group()
        },
    )
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
        let join = if matches!(
            tree.token(previous).kind,
            Token::Eq | Token::BindArrow | Token::FatArrow
        ) {
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

fn data_declaration<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> Option<Document<'a>> {
    let children = tree.children(id);
    let equals = token_position(tree, children, Token::Eq)?;
    if children
        .get(equals + 1..)
        .unwrap_or(&[])
        .iter()
        .any(|child| contains_comment(tree, *child))
    {
        return None;
    }
    let header = joined_children(tree, source, id, slice_to(children, equals), width).0;
    let equals_document =
        joined_children(tree, source, id, slice(children, equals, equals + 1), width).0;
    let constructors = joined_children(
        tree,
        source,
        id,
        children.get(equals + 1..).unwrap_or(&[]),
        width,
    )
    .0;

    Some(
        header
            .append(
                BoxDoc::line()
                    .append(equals_document)
                    .append(BoxDoc::space())
                    .append(constructors)
                    .nest(INDENT),
            )
            .group(),
    )
}

fn operator_chain<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> Option<Document<'a>> {
    let children = tree.children(id);
    if children.iter().any(|child| contains_comment(tree, *child)) {
        return None;
    }
    let operators = children
        .iter()
        .enumerate()
        .filter_map(|(position, child)| match child {
            NodeOrToken::Node(child)
                if tree.node(*child).kind() == SyntaxKind::BacktickOperator =>
            {
                Some(position)
            }
            NodeOrToken::Token(token)
                if matches!(
                    tree.token(*token).kind,
                    Token::Operator | Token::ReservedOp | Token::Cons | Token::Dot | Token::Minus
                ) =>
            {
                Some(position)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let first_operator = *operators.first()?;

    let mut document =
        joined_children(tree, source, id, slice_to(children, first_operator), width).0;
    for (index, operator_position) in operators.iter().copied().enumerate() {
        let operand_end = operators.get(index + 1).copied().unwrap_or(children.len());
        let operator = joined_children(
            tree,
            source,
            id,
            slice(children, operator_position, operator_position + 1),
            width,
        );
        let operand = joined_children(
            tree,
            source,
            id,
            slice(children, operator_position + 1, operand_end),
            width,
        );
        operand.1?;
        let trailing = operator
            .1
            .is_some_and(|token| tree.token(token).source_text(source) == "$");
        document = if trailing {
            document
                .append(BoxDoc::space())
                .append(operator.0)
                .append(BoxDoc::line().append(operand.0).nest(INDENT))
        } else {
            document.append(
                BoxDoc::line()
                    .append(operator.0)
                    .append(BoxDoc::space())
                    .append(operand.0)
                    .nest(INDENT),
            )
        };
    }

    Some(document.group())
}

fn block<'a>(tree: &'a SyntaxTree, source: &'a str, id: CstId, width: usize) -> Document<'a> {
    let kind = tree.node(id).kind();
    let children = tree.children(id);
    let last_header = if matches!(
        kind,
        SyntaxKind::Module | SyntaxKind::ModuleDirectiveList | SyntaxKind::ImportList
    ) {
        children.iter().rposition(|child| {
            matches!(
                child,
                NodeOrToken::Node(child)
                    if matches!(
                        tree.node(*child).kind(),
                        SyntaxKind::ModuleDirectiveList
                            | SyntaxKind::ExportDecl
                            | SyntaxKind::PrivateDecl
                            | SyntaxKind::ReexportDecl
                            | SyntaxKind::ImportList
                            | SyntaxKind::ImportDecl
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
                && last_header.is_some_and(|last_header| position > last_header)
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
                layout_block_part(
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
        layout_block_part(tree, source, id, slice(children, start, body_end), width),
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

fn layout_block_part<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    parent: CstId,
    children: &[NodeOrToken],
    width: usize,
) -> BoundedDocument<'a> {
    if tree.node(parent).kind() == SyntaxKind::InstanceDef {
        instance_equation(tree, source, parent, children, width)
            .unwrap_or_else(|| joined_children(tree, source, parent, children, width))
    } else {
        joined_children(tree, source, parent, children, width)
    }
}

fn instance_equation<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    parent: CstId,
    children: &[NodeOrToken],
    width: usize,
) -> Option<BoundedDocument<'a>> {
    if children.iter().any(|child| contains_comment(tree, *child)) {
        return None;
    }
    let equals = token_position(tree, children, Token::Eq)?;
    let left_children = children.get(..equals).unwrap_or(&[]);
    let inline_left = joined_children(tree, source, parent, left_children, usize::MAX).0;
    let inline_left = render_unbounded(&inline_left);
    let left =
        if inline_left.chars().count() + DEFINITION_EQUALS_WIDTH + usize::try_from(INDENT).ok()?
            <= width
        {
            BoxDoc::text(inline_left)
        } else {
            let parts = declaration_head_parts(tree, source, parent, left_children, width);
            BoxDoc::intersperse(parts, BoxDoc::hardline())
        };
    let equals_document = joined_children(
        tree,
        source,
        parent,
        slice(children, equals, equals + 1),
        width,
    )
    .0;
    let body_children = children.get(equals + 1..).unwrap_or(&[]);
    let body = joined_children(tree, source, parent, body_children, width);
    body.1?;
    let header = left.append(BoxDoc::space()).append(equals_document);
    let document = if body_children
        .iter()
        .any(|child| starts_with_do_expression(tree, *child))
    {
        header.append(BoxDoc::space()).append(body.0)
    } else {
        header.append(BoxDoc::line().append(body.0)).group()
    };
    let first = children
        .iter()
        .find_map(|child| first_boundary(tree, *child));
    let last = children
        .iter()
        .rev()
        .find_map(|child| last_boundary(tree, *child));
    Some((document, first, last))
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
    let initializer_children = slice(children, equals + 1, in_keyword);
    let initializer = joined_children(tree, source, id, initializer_children, width).0;
    let continuation = joined_children(
        tree,
        source,
        id,
        children.get(in_keyword..).unwrap_or(&[]),
        width,
    )
    .0;

    let formatted_initializer = if initializer_children
        .iter()
        .any(|child| starts_with_do_expression(tree, *child))
    {
        BoxDoc::space().append(initializer)
    } else {
        BoxDoc::line().append(initializer).nest(INDENT)
    };

    header
        .append(formatted_initializer)
        .append(BoxDoc::line())
        .append(continuation)
        .group()
}

fn if_expression<'a>(
    tree: &'a SyntaxTree,
    source: &'a str,
    id: CstId,
    width: usize,
) -> Option<Document<'a>> {
    let children = tree.children(id);
    if children.iter().any(|child| contains_comment(tree, *child)) {
        return None;
    }
    let then_keyword = token_position(tree, children, Token::Then)?;
    let else_keyword = token_position_after(tree, children, Token::Else, then_keyword + 1)?;
    let condition = joined_children(tree, source, id, slice_to(children, then_keyword), width).0;
    let then_token = joined_children(
        tree,
        source,
        id,
        slice(children, then_keyword, then_keyword + 1),
        width,
    )
    .0;
    let then_body = joined_children(
        tree,
        source,
        id,
        slice(children, then_keyword + 1, else_keyword),
        width,
    )
    .0;
    let else_token = joined_children(
        tree,
        source,
        id,
        slice(children, else_keyword, else_keyword + 1),
        width,
    )
    .0;
    let else_children = children.get(else_keyword + 1..).unwrap_or(&[]);
    let else_body = joined_children(tree, source, id, else_children, width).0;
    let then_clause = then_token
        .append(BoxDoc::line().append(then_body).nest(INDENT))
        .group();
    let else_separator = if else_children
        .iter()
        .any(|child| starts_with_if_or_layout(tree, *child))
    {
        BoxDoc::space()
    } else {
        BoxDoc::line()
    };
    let else_clause = else_token
        .append(else_separator.append(else_body).nest(INDENT))
        .group();

    Some(
        condition
            .append(BoxDoc::line().append(then_clause).nest(INDENT))
            .append(BoxDoc::line().append(else_clause).nest(INDENT))
            .group(),
    )
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
                    | SyntaxKind::PredicateList
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
