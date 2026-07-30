use std::env;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use solveig_base::{Diagnostic, Session, Span};
use solveig_syntax::cst::{CstId, NodeOrToken, SyntaxTree, TokenId};
use solveig_syntax::lexer::{PosToken, Token};
use solveig_syntax::parser::parse_source;

#[test]
fn golden_cst_cases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cst");
    let update = env::var("UPDATE_GOLDEN").is_ok_and(|value| value == "1");

    let mut cases = golden_cases(&root);
    cases.sort();

    assert!(
        !cases.is_empty(),
        "no CST golden cases found under {}",
        root.display()
    );

    for case_root in cases {
        let case_name = case_name(&case_root);
        let source_path = case_root.join("source.sv");
        let source = fs::read_to_string(&source_path).unwrap_or_else(|err| {
            panic!(
                "failed to read CST source for `{case_name}` at {}: {err}",
                source_path.display()
            )
        });

        let session = Session::new();
        let file = session.add_file(source_path.clone(), source.clone());
        let parsed = parse_source(&session, file, &source);
        let actual = dump_cst(&parsed.cst, &source);

        assert_eq!(
            parsed.cst.reconstruct_text(&source),
            source,
            "CST golden case `{case_name}` does not reconstruct its source"
        );

        let expected_path = case_root.join("expected.txt");
        if update {
            fs::write(&expected_path, &actual).unwrap_or_else(|err| {
                panic!(
                    "failed to update CST golden for `{case_name}` at {}: {err}",
                    expected_path.display()
                )
            });
            continue;
        }

        let expected = fs::read_to_string(&expected_path).unwrap_or_else(|err| {
            panic!(
                "failed to read expected CST dump for `{case_name}` at {}: {err}\n\
                 run with UPDATE_GOLDEN=1 to create or refresh it",
                expected_path.display()
            )
        });

        assert_eq!(
            actual,
            expected,
            "CST golden case `{case_name}` changed:\n{}",
            diff_preview(&expected, &actual)
        );
    }
}

fn dump_cst(tree: &SyntaxTree, source: &str) -> String {
    let mut out = String::new();
    dump_node(tree, source, tree.root(), 0, &mut out);
    out
}

fn dump_node(tree: &SyntaxTree, source: &str, id: CstId, depth: usize, out: &mut String) {
    let node = tree.node(id);
    write_indent(out, depth);
    writeln!(out, "{:?} {}", node.kind(), span_label(node.span()))
        .expect("writing to String should not fail");

    for diagnostic in node.diagnostics() {
        dump_diagnostic(diagnostic, depth + 1, out);
    }

    for child in node.children() {
        match child {
            NodeOrToken::Node(id) => dump_node(tree, source, *id, depth + 1, out),
            NodeOrToken::Token(id) => dump_token(tree, *id, source, depth + 1, out),
        }
    }
}

fn dump_diagnostic(diagnostic: &Diagnostic, depth: usize, out: &mut String) {
    write_indent(out, depth);
    writeln!(
        out,
        "diagnostic {} {:?} {}",
        diagnostic.code,
        diagnostic.severity,
        quoted(&diagnostic.message)
    )
    .expect("writing to String should not fail");

    for label in &diagnostic.labels {
        write_indent(out, depth + 1);
        let kind = if label.primary {
            "primary"
        } else {
            "secondary"
        };
        writeln!(
            out,
            "label {kind} {} {}",
            span_label(label.span.span),
            quoted(&label.message)
        )
        .expect("writing to String should not fail");
    }

    for note in &diagnostic.notes {
        write_indent(out, depth + 1);
        writeln!(out, "note {}", quoted(note)).expect("writing to String should not fail");
    }
}

fn dump_token(tree: &SyntaxTree, id: TokenId, source: &str, depth: usize, out: &mut String) {
    let token = tree.token(id);
    write_indent(out, depth);
    write!(
        out,
        "{} {} line={} col={}",
        token_label(&token.kind),
        span_label(Span::from_usize_range(token.start..token.end)),
        token.line,
        token.col
    )
    .expect("writing to String should not fail");

    if token.is_synthetic() {
        out.push_str(" synthetic");
    }

    writeln!(out, " text={}", quoted(token.source_text(source)))
        .expect("writing to String should not fail");

    for comment in tree.leading_comments(id) {
        dump_attached_comment(
            "leading-comment",
            tree.token(*comment),
            source,
            depth + 1,
            out,
        );
    }

    for comment in tree.trailing_comments(id) {
        dump_attached_comment(
            "trailing-comment",
            tree.token(*comment),
            source,
            depth + 1,
            out,
        );
    }
}

fn dump_attached_comment(
    label: &str,
    token: &PosToken,
    source: &str,
    depth: usize,
    out: &mut String,
) {
    write_indent(out, depth);
    writeln!(
        out,
        "{} {} text={}",
        label,
        span_label(Span::from_usize_range(token.start..token.end)),
        quoted(token.source_text(source))
    )
    .expect("writing to String should not fail");
}

fn token_label(token: &Token) -> String {
    match token {
        Token::Ident(_) => "Ident".to_string(),
        Token::UpperIdent(_) => "UpperIdent".to_string(),
        Token::Operator(_) => "Operator".to_string(),
        Token::ReservedOp(_) => "ReservedOp".to_string(),
        other => format!("{other:?}"),
    }
}

fn span_label(span: Span) -> String {
    format!("@{}..{}", span.start, span.end)
}

fn quoted(text: &str) -> String {
    format!("{text:?}")
}

fn write_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn golden_cases(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .unwrap_or_else(|err| panic!("failed to read CST golden root {}: {err}", root.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("failed to read CST golden directory entry: {err}"))
                .path()
        })
        .filter(|path| path.join("source.sv").is_file())
        .collect()
}

fn case_name(case_root: &Path) -> &str {
    case_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<invalid-case-name>")
}

fn diff_preview(expected: &str, actual: &str) -> String {
    let first_diff = expected
        .bytes()
        .zip(actual.bytes())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.len().min(actual.len()));

    format!(
        "expected len: {}\nactual len:   {}\nfirst diff byte: {}\nexpected: {}\nactual:   {}",
        expected.len(),
        actual.len(),
        first_diff,
        preview_around(expected, first_diff),
        preview_around(actual, first_diff)
    )
}

fn preview_around(text: &str, byte_index: usize) -> String {
    let start = text[..byte_index.min(text.len())]
        .char_indices()
        .rev()
        .nth(80)
        .map_or(0, |(index, _)| index);

    let end = text[byte_index.min(text.len())..]
        .char_indices()
        .nth(160)
        .map_or(text.len(), |(index, _)| byte_index.min(text.len()) + index);

    let prefix = if start > 0 { "..." } else { "" };
    let suffix = if end < text.len() { "..." } else { "" };

    format!("{prefix}{:?}{suffix}", &text[start..end])
}
