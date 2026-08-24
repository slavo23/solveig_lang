use std::collections::BTreeSet;
use std::env;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use solveig_base::{FileSpan, SourceMap};
use solveig_driver::Compilation;
use solveig_ide::{
    CompilationIdeExt, SourceSemanticToken, SourceSemanticTokenKind, SourceSemanticTokenModifier,
};

use crate::snapshot_support;

#[derive(Debug)]
struct DecodedToken {
    text: String,
    kind: SourceSemanticTokenKind,
    modifiers: Vec<SourceSemanticTokenModifier>,
}

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

#[test]
fn semantic_tokens_golden_decoded_stream() {
    let project_root = fixture_root("tests/fixtures/semantic_tokens");
    let main_file = project_root.join("src/main.sv");
    let mut compilation = Compilation::new(&project_root);
    let tokens = compilation.semantic_tokens(&main_file);
    let decoded = decode_tokens(&tokens, compilation.source_map());
    let actual = dump_tokens(&tokens, compilation.source_map());
    let expected_path = fixture_root("tests/golden/semantic_tokens/h15/expected.txt");
    let expected = fs::read_to_string(&expected_path).unwrap_or_else(|err| {
        panic!(
            "failed to read semantic-token golden at {}: {err}\nactual:\n{actual}",
            expected_path.display(),
        )
    });

    snapshot_support::assert_fixture_snapshot(
        expected_path
            .parent()
            .expect("semantic-token golden should have a parent"),
        "semantic_tokens_h15",
        &expected,
        &actual,
    );
    assert_sorted_and_non_overlapping(&tokens);
    assert_all_kinds_covered(&decoded);
    assert_all_modifiers_covered(&decoded);
    assert_has(
        &decoded,
        "semanticLib",
        SourceSemanticTokenKind::Namespace,
        &[],
    );
    assert_has(&decoded, "data", SourceSemanticTokenKind::Keyword, &[]);
    assert_has(
        &decoded,
        "-- semantic coverage comment",
        SourceSemanticTokenKind::Comment,
        &[],
    );
    assert_has(
        &decoded,
        "LocalBox",
        SourceSemanticTokenKind::Type,
        &[
            SourceSemanticTokenModifier::Declaration,
            SourceSemanticTokenModifier::Definition,
        ],
    );
    assert_has(
        &decoded,
        "a",
        SourceSemanticTokenKind::TypeParameter,
        &[SourceSemanticTokenModifier::Declaration],
    );
    assert_has(
        &decoded,
        "LocalShow",
        SourceSemanticTokenKind::Class,
        &[
            SourceSemanticTokenModifier::Declaration,
            SourceSemanticTokenModifier::Definition,
        ],
    );
    assert_has(
        &decoded,
        "localShow",
        SourceSemanticTokenKind::Method,
        &[
            SourceSemanticTokenModifier::Declaration,
            SourceSemanticTokenModifier::Definition,
        ],
    );
    assert_has(
        &decoded,
        "identity",
        SourceSemanticTokenKind::Function,
        &[
            SourceSemanticTokenModifier::Declaration,
            SourceSemanticTokenModifier::Definition,
        ],
    );
    assert_has(
        &decoded,
        "value",
        SourceSemanticTokenKind::Parameter,
        &[SourceSemanticTokenModifier::Declaration],
    );
    assert_has(&decoded, "value", SourceSemanticTokenKind::Variable, &[]);
    assert_has(
        &decoded,
        "MkLocalBox",
        SourceSemanticTokenKind::EnumMember,
        &[
            SourceSemanticTokenModifier::Declaration,
            SourceSemanticTokenModifier::Definition,
        ],
    );
    assert_has(&decoded, "+", SourceSemanticTokenKind::Operator, &[]);
    assert_has(
        &decoded,
        "\"solveig\"",
        SourceSemanticTokenKind::String,
        &[],
    );
    assert_has(&decoded, "42", SourceSemanticTokenKind::Number, &[]);
    assert_has(
        &decoded,
        "pure",
        SourceSemanticTokenKind::Method,
        &[SourceSemanticTokenModifier::DefaultLibrary],
    );
}

#[test]
fn semantic_tokens_mid_edit_unparseable_file_golden() {
    let case_root = fixture_root("tests/golden/semantic_tokens/mid_edit");
    let source_file = case_root.join("source.sv");
    let mut compilation = Compilation::for_source_file(&source_file);
    let tokens = compilation.semantic_tokens(&source_file);
    let decoded = decode_tokens(&tokens, compilation.source_map());
    let actual = dump_tokens(&tokens, compilation.source_map());
    let expected_path = case_root.join("expected.txt");
    let update = env::var("UPDATE_GOLDEN").is_ok_and(|value| value == "1");

    let expected = if update {
        fs::write(&expected_path, &actual).unwrap_or_else(|error| {
            panic!(
                "failed to update mid-edit semantic-token golden at {}: {error}",
                expected_path.display()
            )
        });
        actual.clone()
    } else {
        fs::read_to_string(&expected_path).unwrap_or_else(|error| {
            panic!(
                "failed to read mid-edit semantic-token golden at {}: {error}\n\
                 run with UPDATE_GOLDEN=1 to create or refresh it",
                expected_path.display()
            )
        })
    };

    snapshot_support::assert_fixture_snapshot(
        &case_root,
        "semantic_tokens_mid_edit",
        &expected,
        &actual,
    );
    assert_sorted_and_non_overlapping(&tokens);
    assert_has(&decoded, "if", SourceSemanticTokenKind::Keyword, &[]);
    assert_has(&decoded, "then", SourceSemanticTokenKind::Keyword, &[]);
}

fn decode_tokens(tokens: &[SourceSemanticToken], source_map: &SourceMap) -> Vec<DecodedToken> {
    tokens
        .iter()
        .map(|token| {
            let span = token.file_span();
            let source = source_map.lookup(span.file);
            DecodedToken {
                text: source
                    .src
                    .get(span.span.to_usize_range())
                    .expect("semantic token span should slice source")
                    .to_string(),
                kind: token.kind(),
                modifiers: token.modifiers().iter().collect(),
            }
        })
        .collect()
}

fn dump_tokens(tokens: &[SourceSemanticToken], source_map: &SourceMap) -> String {
    let mut out = String::new();

    for token in tokens {
        let span = token.file_span();
        let source = source_map.lookup(span.file);
        let (line, col) = source_map.line_col(span.file, span.span.start);
        let (end_line, end_col) = source_map.line_col(span.file, span.span.end);
        let text = source
            .src
            .get(span.span.to_usize_range())
            .expect("semantic token span should slice source")
            .escape_debug();
        let modifiers = token
            .modifiers()
            .iter()
            .map(|modifier| format!("{modifier:?}"))
            .collect::<Vec<_>>()
            .join("|");

        writeln!(
            out,
            "{}:{}-{}:{} {:?} [{}] `{}`",
            line + 1,
            col + 1,
            end_line + 1,
            end_col + 1,
            token.kind(),
            if modifiers.is_empty() {
                "-"
            } else {
                &modifiers
            },
            text
        )
        .expect("writing semantic-token golden to a String should not fail");
    }

    out
}

fn assert_has(
    decoded: &[DecodedToken],
    text: &str,
    kind: SourceSemanticTokenKind,
    modifiers: &[SourceSemanticTokenModifier],
) {
    assert!(
        decoded.iter().any(|token| {
            token.text == text
                && token.kind == kind
                && modifiers
                    .iter()
                    .all(|modifier| token.modifiers.contains(modifier))
        }),
        "expected semantic token `{text}` with kind {kind:?} and modifiers {modifiers:?}"
    );
}

fn assert_all_kinds_covered(decoded: &[DecodedToken]) {
    let actual = decoded
        .iter()
        .map(|token| token.kind)
        .collect::<BTreeSet<_>>();
    let expected = BTreeSet::from([
        SourceSemanticTokenKind::Namespace,
        SourceSemanticTokenKind::Type,
        SourceSemanticTokenKind::Class,
        SourceSemanticTokenKind::TypeParameter,
        SourceSemanticTokenKind::Parameter,
        SourceSemanticTokenKind::Variable,
        SourceSemanticTokenKind::Function,
        SourceSemanticTokenKind::EnumMember,
        SourceSemanticTokenKind::Method,
        SourceSemanticTokenKind::Keyword,
        SourceSemanticTokenKind::Operator,
        SourceSemanticTokenKind::String,
        SourceSemanticTokenKind::Number,
        SourceSemanticTokenKind::Comment,
    ]);

    pretty_assertions::assert_eq!(actual, expected);
}

fn assert_all_modifiers_covered(decoded: &[DecodedToken]) {
    let actual = decoded
        .iter()
        .flat_map(|token| token.modifiers.iter().copied())
        .collect::<BTreeSet<_>>();
    let expected = BTreeSet::from([
        SourceSemanticTokenModifier::Declaration,
        SourceSemanticTokenModifier::Definition,
        SourceSemanticTokenModifier::Readonly,
        SourceSemanticTokenModifier::DefaultLibrary,
    ]);

    pretty_assertions::assert_eq!(actual, expected);
}

fn assert_sorted_and_non_overlapping(tokens: &[SourceSemanticToken]) {
    let mut previous: Option<FileSpan> = None;

    for token in tokens {
        let span = token.file_span();
        if let Some(previous) = previous
            && previous.file == span.file
        {
            assert!(
                previous.span.end <= span.span.start,
                "semantic tokens should not overlap: {previous:?} then {span:?}"
            );
        }
        previous = Some(span);
    }
}
