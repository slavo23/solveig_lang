//! CST-backed editor features that do not require semantic resolution.

use super::*;
use solveig_syntax::cst::{NodeOrToken, SyntaxTree};
use solveig_syntax::lexer::Token;
use solveig_syntax::syntax::SyntaxKind;

#[derive(Clone, Debug)]
pub(super) struct SourceSyntaxTree {
    file: FileId,
    cst: SyntaxTree,
}

impl SourceSyntaxTree {
    fn new(file: FileId, cst: SyntaxTree) -> Self {
        Self { file, cst }
    }
}

pub(super) fn project_syntax_trees(project: &Project) -> Vec<SourceSyntaxTree> {
    project
        .modules
        .values()
        .map(|module_file| {
            SourceSyntaxTree::new(module_file.parsed.file, module_file.parsed.cst.clone())
        })
        .collect()
}

pub(super) fn folding_ranges_in_syntax_trees(
    source_map: &SourceMap,
    syntax_trees: &[SourceSyntaxTree],
    file: &Path,
) -> Vec<SourceFoldingRange> {
    let Some(file_id) = source_lookup::source_file_id_in_source_map(source_map, file) else {
        return Vec::new();
    };
    let Some(tree) = syntax_tree_for_file(syntax_trees, file_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    collect_syntax_folds(source_map, tree, tree.cst.root(), &mut out);
    collect_comment_folds(source_map, tree, &mut out);
    out.sort_by_key(folding_range_sort_key);
    out.dedup_by(|left, right| {
        left.file_span() == right.file_span() && left.kind() == right.kind()
    });
    out
}

fn collect_syntax_folds(
    source_map: &SourceMap,
    tree: &SourceSyntaxTree,
    id: solveig_syntax::cst::CstId,
    out: &mut Vec<SourceFoldingRange>,
) {
    let node = tree.cst.node(id);

    if foldable_syntax_kind(node.kind()) && multiline(source_map, tree.file, node.span()) {
        out.push(SourceFoldingRange::new(
            FileSpan::new(tree.file, node.span()),
            SourceFoldingRangeKind::Region,
        ));
    }

    for child in tree.cst.children(id) {
        if let NodeOrToken::Node(child) = child {
            collect_syntax_folds(source_map, tree, *child, out);
        }
    }
}

fn collect_comment_folds(
    source_map: &SourceMap,
    tree: &SourceSyntaxTree,
    out: &mut Vec<SourceFoldingRange>,
) {
    let mut line_comments = Vec::<(u32, Span)>::new();

    for token in tree.cst.tokens() {
        match token.kind {
            Token::LineComment => line_comments.push((token.line, token_span(token))),
            Token::BlockComment if multiline(source_map, tree.file, token_span(token)) => {
                out.push(SourceFoldingRange::new(
                    FileSpan::new(tree.file, token_span(token)),
                    SourceFoldingRangeKind::Comment,
                ));
            }
            _ => {}
        }
    }

    let mut index = 0;
    while index < line_comments.len() {
        let (start_line, start_span) = *line_comments
            .get(index)
            .expect("ICE: line comment group index out of bounds");
        let mut last_line = start_line;
        let mut end_span = start_span;
        let mut next = index + 1;

        while next < line_comments.len() {
            let (next_line, next_span) = *line_comments
                .get(next)
                .expect("ICE: next line comment index out of bounds");
            if next_line != last_line.saturating_add(1) {
                break;
            }
            last_line = next_line;
            end_span = next_span;
            next += 1;
        }

        if last_line != start_line {
            let span = Span::new(start_span.start, end_span.end);
            out.push(SourceFoldingRange::new(
                FileSpan::new(tree.file, span),
                SourceFoldingRangeKind::Comment,
            ));
        }

        index = next;
    }
}

fn foldable_syntax_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Signature
            | SyntaxKind::FunctionDef
            | SyntaxKind::FixityDef
            | SyntaxKind::DataDef
            | SyntaxKind::TypeAliasDef
            | SyntaxKind::ClassDef
            | SyntaxKind::InstanceDef
            | SyntaxKind::DoExpr
            | SyntaxKind::CaseExpr
    )
}

fn multiline(source_map: &SourceMap, file: FileId, span: Span) -> bool {
    let (start_line, _) = source_map.line_col(file, span.start);
    let (end_line, _) = source_map.line_col(file, span.end);

    start_line < end_line
}

fn syntax_tree_for_file(
    syntax_trees: &[SourceSyntaxTree],
    file: FileId,
) -> Option<&SourceSyntaxTree> {
    syntax_trees.iter().find(|tree| tree.file == file)
}

fn token_span(token: &solveig_syntax::lexer::PosToken) -> Span {
    Span::from_usize_range(token.start()..token.end())
}

fn folding_range_sort_key(
    range: &SourceFoldingRange,
) -> (FileId, u32, u32, SourceFoldingRangeKind) {
    let span = range.file_span();

    (span.file, span.span.start, span.span.end, range.kind())
}
