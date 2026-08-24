//! Deterministic source formatting from Solveig's lossless concrete syntax tree.

use pretty::BoxDoc;
use solveig_syntax::cst::{CstId, NodeOrToken, SyntaxTree};
use std::ops::Range;

mod document;

/// Formats one parsed source file at the requested maximum line width.
///
/// `source` must be the exact text from which `tree` was built. The formatter
/// never reparses or resolves the program: all structure and trivia ownership
/// come from the lossless CST.
pub fn format(tree: &SyntaxTree, source: &str, width: usize) -> String {
    render_document(&document::root(tree, source, width), width, true)
}

/// Formats one CST subtree and returns the exact source range owned by it.
///
/// This is intended for structural editor operations such as range formatting.
/// The returned range includes trivia stored inside the subtree, so replacing it
/// cannot duplicate an inline comment that extends beyond the node's semantic
/// span. It deliberately does not include leading trivia owned by the parent.
pub fn format_subtree(
    tree: &SyntaxTree,
    source: &str,
    node: CstId,
    width: usize,
) -> Option<(Range<usize>, String)> {
    let range = concrete_range(tree, node)?;
    let formatted = render_document(&document::node(tree, source, node, width).0, width, false);
    Some((range, formatted))
}

fn render_document(document: &BoxDoc<'_>, width: usize, final_newline: bool) -> String {
    let mut output = Vec::new();

    // Following Wadler (1998) and Lindig (2000), document construction is
    // separate from strict width-sensitive rendering. `Vec<u8>` writes cannot
    // fail, and all source tokens are already valid UTF-8 slices.
    document
        .render(width, &mut output)
        .expect("ICE: rendering a formatter document into memory must succeed");

    let mut formatted =
        String::from_utf8(output).expect("ICE: formatter documents must contain valid UTF-8");
    while formatted.ends_with('\n') {
        let _ = formatted.pop();
    }
    let mut normalized = formatted
        .lines()
        .map(|line| if line.trim().is_empty() { "" } else { line })
        .collect::<Vec<_>>()
        .join("\n");
    if final_newline {
        normalized.push('\n');
    }
    normalized
}

fn concrete_range(tree: &SyntaxTree, node: CstId) -> Option<Range<usize>> {
    let mut range: Option<Range<usize>> = None;
    let mut pending = vec![node];

    while let Some(node) = pending.pop() {
        for child in tree.children(node) {
            match child {
                NodeOrToken::Node(child) => pending.push(*child),
                NodeOrToken::Token(token) => {
                    let token = tree.token(*token);
                    let token_range = token.start()..token.end();
                    range = Some(match range {
                        Some(range) => {
                            range.start.min(token_range.start)..range.end.max(token_range.end)
                        }
                        None => token_range,
                    });
                }
            }
        }
    }

    range
}

#[cfg(test)]
mod tests;
