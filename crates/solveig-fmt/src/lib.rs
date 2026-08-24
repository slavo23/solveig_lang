//! Deterministic source formatting from Solveig's lossless concrete syntax tree.

use pretty::BoxDoc;
use solveig_syntax::cst::SyntaxTree;

mod document;

/// Formats one parsed source file at the requested maximum line width.
///
/// `source` must be the exact text from which `tree` was built. The formatter
/// never reparses or resolves the program: all structure and trivia ownership
/// come from the lossless CST.
pub fn format(tree: &SyntaxTree, source: &str, width: usize) -> String {
    let mut output = Vec::new();
    let document: BoxDoc<'_> = document::root(tree, source, width);

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
    normalized.push('\n');
    normalized
}

#[cfg(test)]
mod tests;
