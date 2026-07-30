use super::*;

use std::fs;
use std::path::{Path, PathBuf};

use solveig_base::{FileId, Session};

fn lex_test_source(source: &str) -> (Session, FileId, Vec<PosToken>) {
    let session = Session::new();
    let file = session.add_file("test.sv", source);
    let tokens = lex(file, source).expect("test source should lex");

    (session, file, tokens)
}

fn reconstruct(tokens: &[PosToken], source: &str) -> String {
    tokens
        .iter()
        .map(|token| token.source_text(source))
        .collect()
}

#[test]
fn trivia_tokens_reconstruct_source_and_parser_view_skips_them() {
    let source = "main : IO ()\nmain = do\n    -- keep me\n\n    printLine \"x\"\n";
    let (_session, _file, tokens) = lex_test_source(source);

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
    let (_session, _file, tokens) = lex_test_source(source);
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
fn all_repository_solveig_sources_round_trip_through_tokens() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("syntax crate should live under crates/");
    let mut files = Vec::new();
    collect_solveig_sources(repo, &mut files);
    files.sort();

    assert!(!files.is_empty(), "expected repository .sv files");

    let session = Session::new();
    for path in files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let file = session.add_file(&path, source.clone());
        let tokens = lex(file, &source).unwrap_or_else(|err| panic!("{}: {err:?}", path.display()));

        assert_eq!(
            reconstruct(&tokens, &source),
            source,
            "token reconstruction changed {}",
            path.display()
        );
    }
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
