use super::*;

use std::fs;
use std::path::{Path, PathBuf};

use solveig_base::FileId;

fn lex_test_source(source: &str) -> Vec<PosToken> {
    lex(FileId(0), source).expect("test source should lex")
}

fn reconstruct(tokens: &[PosToken], source: &str) -> String {
    tokens
        .iter()
        .map(|token| token.source_text(source))
        .collect()
}

#[test]
fn trivia_tokens_reconstruct_source_and_parser_view_skips_them() {
    let source = "main : Task ()\nmain = do\n    -- keep me\n\n    printLine \"x\"\n";
    let tokens = lex_test_source(source);

    assert_eq!(reconstruct(&tokens, source), source);
    assert!(tokens.iter().any(|token| token.kind == Token::Whitespace));
    assert!(
        tokens
            .iter()
            .any(|token| token.kind == Token::Newline && token.source_text(source) == "\n\n")
    );
    assert!(
        tokens
            .iter()
            .any(|token| token.kind == Token::LineComment
                && token.source_text(source) == "-- keep me")
    );
    assert!(tokens.iter().any(|token| token.kind == Token::VOpen
        && token.is_synthetic()
        && token.source_text(source).is_empty()));
    assert!(tokens.iter().any(|token| token.kind == Token::VSemi
        && token.is_synthetic()
        && token.source_text(source).is_empty()));

    let significant = parser_tokens(tokens);
    assert!(significant.iter().all(|token| !token.kind.is_trivia()));
}

#[test]
fn nested_block_comment_is_one_lossless_trivia_token() {
    let source = "main = {- outer {- inner -}\n still outer -} ()\n";
    let tokens = lex_test_source(source);
    let block = tokens
        .iter()
        .find(|token| token.kind == Token::BlockComment)
        .expect("block comment should be retained");

    assert_eq!(
        block.source_text(source),
        "{- outer {- inner -}\n still outer -}"
    );
    assert_eq!(reconstruct(&tokens, source), source);
}

#[test]
fn ordinary_operator_spellings_share_the_generic_operator_token() {
    let source = "+ * / % == < > <= >= <$> <*> >>= <> $";
    let tokens = parser_tokens(lex_test_source(source));
    let operators = tokens
        .iter()
        .filter(|token| !token.is_synthetic() && !matches!(token.kind, Token::Eof))
        .collect::<Vec<_>>();

    assert!(!operators.is_empty());
    assert!(operators.iter().all(|token| token.kind == Token::Operator));
}

#[test]
fn all_repository_solveig_sources_round_trip_through_tokens() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("syntax crate should live under crates/");
    let mut files = Vec::new();
    collect_solveig_sources(repo, &mut files);
    files.sort();

    assert!(!files.is_empty(), "expected repository .sv files");

    for path in files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let tokens =
            lex(FileId(0), &source).unwrap_or_else(|err| panic!("{}: {err:?}", path.display()));

        assert_eq!(
            reconstruct(&tokens, &source),
            source,
            "token reconstruction changed {}",
            path.display()
        );
    }
}

#[test]
fn token_and_positioned_token_are_copy() {
    fn assert_copy<T: Copy>() {}

    assert_copy::<Token>();
    assert_copy::<PosToken>();
}

fn collect_solveig_sources(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap_or_else(|err| {
        panic!("failed to read directory {}: {err}", root.display());
    }) {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();
        let file_name = path.file_name().and_then(|name| name.to_str());

        if matches!(file_name, Some(".git" | "target")) {
            continue;
        }

        if path.is_dir() {
            collect_solveig_sources(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "sv") {
            files.push(path);
        }
    }
}
