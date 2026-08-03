use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use solveig_base::{FileSpan, SourceMap};
use solveig_driver::Compilation;
use solveig_ide::{
    CompilationIdeExt, SourceSemanticToken, SourceSemanticTokenKind, SourceSemanticTokenModifier,
};

#[derive(Debug)]
struct DecodedToken {
    text: String,
    kind: SourceSemanticTokenKind,
    modifiers: Vec<SourceSemanticTokenModifier>,
}

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn temp_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "solveig_semantic_tokens_{name}_{}_{}",
        process::id(),
        nanos
    ))
}

fn write_source(path: &Path, source: &str) {
    fs::create_dir_all(
        path.parent()
            .expect("test source should have a parent directory"),
    )
    .expect("test source directory should be creatable");
    fs::write(path, source).expect("test source should be writable");
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

    assert_eq!(actual, expected);
    assert_sorted_and_non_overlapping(&tokens);
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
fn semantic_tokens_survive_mid_edit_parse_errors() {
    let root = temp_root("broken");
    let source_file = root.join("scratch.sv");
    write_source(
        &source_file,
        r#"main : Int
main = if True then
"#,
    );
    let mut compilation = Compilation::for_source_file(&source_file);
    let tokens = compilation.semantic_tokens(&source_file);
    let decoded = decode_tokens(&tokens, compilation.source_map());

    assert_sorted_and_non_overlapping(&tokens);
    assert_has(&decoded, "if", SourceSemanticTokenKind::Keyword, &[]);
    assert_has(&decoded, "then", SourceSemanticTokenKind::Keyword, &[]);

    let _ = fs::remove_dir_all(root);
}

fn decode_tokens(tokens: &[SourceSemanticToken], source_map: &SourceMap) -> Vec<DecodedToken> {
    tokens
        .iter()
        .map(|token| {
            let span = token.file_span();
            let source = source_map.lookup(span.file);
            DecodedToken {
                text: source.src[span.span.to_usize_range()].to_string(),
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
        let text = source.src[span.span.to_usize_range()].escape_debug();
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
