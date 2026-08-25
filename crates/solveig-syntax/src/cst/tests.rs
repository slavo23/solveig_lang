use super::*;

use std::fs;
use std::path::{Path, PathBuf};

use solveig_base::{FileId, Session};

use crate::ast::ParsedModule;
use crate::lexer::{Token, lex};
use crate::lowering::parse_source;
use crate::parser::parse_cst;

fn parse_test_source(source: &str) -> ParsedModule {
    let session = Session::new();
    let file = session.add_file("test.sv", source);

    parse_source(&session, file, source)
}

#[test]
fn session_free_cst_parse_keeps_structural_expr_nodes_and_drops_empty_imports() {
    let source = "main = [1, (2)]\n";
    let (tree, diagnostics) =
        parse_cst(FileId(0), source).expect("session-free CST parse should lex");

    assert!(diagnostics.is_empty(), "unexpected parse diagnostics");
    assert_eq!(tree.reconstruct_text(source), source);
    assert!(
        tree.nodes()
            .all(|(_, node)| node.kind() != SyntaxKind::ImportList),
        "modules without imports should not emit an empty ImportList"
    );
    assert!(
        tree.nodes()
            .any(|(_, node)| node.kind() == SyntaxKind::ListExpr),
        "list literals should have a structural ListExpr node"
    );
    assert!(
        tree.nodes()
            .any(|(_, node)| node.kind() == SyntaxKind::ParenExpr),
        "parenthesized expressions should have a structural ParenExpr node"
    );
}

#[test]
fn cst_retains_tuple_expression_pattern_type_and_constructor_nodes() {
    let source = "\
pick : (Int, (String, Bool)) -> String
pick (count, (name, active)) = name
makeTriple = (,,)
main = (1, (\"nested\", True))
";
    let (tree, diagnostics) = parse_cst(FileId(0), source).expect("tuple CST fixture should lex");

    assert!(diagnostics.is_empty(), "unexpected tuple parse diagnostics");
    assert_eq!(tree.reconstruct_text(source), source);
    for kind in [
        SyntaxKind::TupleExpr,
        SyntaxKind::TuplePattern,
        SyntaxKind::TupleType,
        SyntaxKind::TupleConstructor,
    ] {
        assert!(
            tree.nodes().any(|(_, node)| node.kind() == kind),
            "tuple CST should retain {kind:?}"
        );
    }
    assert_eq!(
        tree.nodes()
            .filter(|(_, node)| node.kind() == SyntaxKind::TupleExpr)
            .count(),
        2,
        "nested tuple expressions should retain both tuple nodes"
    );
    assert_eq!(
        tree.nodes()
            .filter(|(_, node)| node.kind() == SyntaxKind::TuplePattern)
            .count(),
        2,
        "nested tuple patterns should retain both tuple nodes"
    );
}

#[test]
fn tuple_syntax_reports_omissions_and_arities_above_eight() {
    for source in [
        "main = (1, 2, 3, 4, 5, 6, 7, 8, 9)\n",
        "bad : (Int, Int, Int, Int, Int, Int, Int, Int, Int)\n",
        "pick (a, b, c, d, e, f, g, h, i) = a\n",
        "main = (1,)\n",
    ] {
        let (_, diagnostics) =
            parse_cst(FileId(0), source).expect("invalid tuple fixture should still lex");
        assert!(
            !diagnostics.is_empty(),
            "invalid tuple syntax should report a diagnostic: {source}"
        );
    }
}

#[test]
fn lexing_many_identifiers_does_not_touch_session_interner() {
    let source = (0..10_000)
        .map(|index| format!("name{index} = {index}\n"))
        .collect::<String>();
    let session = Session::new();
    let before = session.interned_symbol_count();

    let tokens = lex(FileId(0), &source).expect("generated identifiers should lex");

    assert!(!tokens.is_empty());
    assert_eq!(session.interned_symbol_count(), before);
}

#[test]
fn cst_reconstructs_text_and_supports_navigation() {
    let source = "main : Task ()\nmain = do\n    -- retain\n    printLine \"ok\"\n";
    let parsed = parse_test_source(source);
    let tree = &parsed.cst;

    assert_eq!(tree.reconstruct_text(source), source);
    assert_eq!(tree.file(), parsed.file);
    assert_eq!(tree.node(tree.root()).kind(), SyntaxKind::Module);
    assert!(
        tree.children(tree.root())
            .iter()
            .any(|child| matches!(child, NodeOrToken::Node(id)
                if tree.node(*id).kind() == SyntaxKind::DefinitionList))
    );

    let whitespace_offset = source.find("    --").expect("fixture has indentation");
    let whitespace = tree
        .token_at_offset(whitespace_offset)
        .expect("indentation should be a trivia token");
    assert_eq!(tree.token(whitespace).kind, Token::Whitespace);

    let print_offset = source.find("printLine").expect("fixture has function call");
    let print_token = tree
        .token_at_offset(print_offset)
        .expect("function name should have a token");
    assert!(matches!(tree.token(print_token).kind, Token::Ident));

    let token_parent = tree
        .token_parent(print_token)
        .expect("tokens should have parent nodes");
    assert_eq!(tree.node(token_parent).kind(), SyntaxKind::NamePath);

    let node = tree
        .node_at_offset(print_offset)
        .expect("function name should have a syntax node");
    assert_eq!(tree.node(node).kind(), SyntaxKind::NamePath);

    let parent = tree.parent(node).expect("name path should be nested");
    assert_eq!(tree.node(parent).kind(), SyntaxKind::AppExpr);
}

#[test]
fn cst_retains_error_node_diagnostics() {
    let source = "bad :\nmain = ()\n";
    let parsed = parse_test_source(source);
    let tree = &parsed.cst;

    assert_eq!(tree.reconstruct_text(source), source);

    let (_, error) = tree
        .nodes()
        .find(|(_, node)| node.kind() == SyntaxKind::Error)
        .expect("recovering parser should emit an error node");

    assert!(
        error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "PAR0001")
    );
}

#[test]
fn cst_parent_links_match_child_edges() {
    let parsed = parse_test_source("main : Task ()\nmain = pure ()\n");
    let tree = &parsed.cst;
    let mut seen_tokens = vec![false; tree.tokens().len()];

    for (id, node) in tree.nodes() {
        for child in node.children() {
            match child {
                NodeOrToken::Node(child) => {
                    assert_eq!(tree.parent(*child), Some(id));
                }
                NodeOrToken::Token(token) => {
                    assert_eq!(tree.token_parent(*token), Some(id));
                    seen_tokens[token.index()] = true;
                }
            }
        }
    }

    assert!(seen_tokens.iter().all(|seen| *seen));
}

#[test]
fn cst_node_spans_exclude_leading_trivia() {
    let source = "type Score a = (String, a)\n\nmain : Score Int\nmain = (\"ok\", 1)\n";
    let parsed = parse_test_source(source);
    let tree = &parsed.cst;

    assert_eq!(tree.reconstruct_text(source), source);

    assert_node_span_text(tree, source, SyntaxKind::NamePath, "String", "String");
    assert_node_span_text(tree, source, SyntaxKind::NamePath, "Int", "Int");
    assert_node_span_text(tree, source, SyntaxKind::AtomExpr, "\"ok\"", "\"ok\"");
    assert_node_span_text(tree, source, SyntaxKind::AtomExpr, "1", "1");
}

#[test]
fn cst_places_leading_trivia_at_surrounding_parent_level() {
    let source = "main : IO HostError ()\nmain = pure ()\n";
    let parsed = parse_test_source(source);
    let tree = &parsed.cst;

    let io_offset = source.find("IO").expect("fixture has IO");
    let whitespace_offset = io_offset - 1;
    let whitespace = tree
        .token_at_offset(whitespace_offset)
        .expect("space before IO should be a trivia token");
    let whitespace_parent = tree
        .token_parent(whitespace)
        .expect("trivia tokens should stay placed in the CST");

    assert_eq!(tree.token(whitespace).kind, Token::Whitespace);
    assert_eq!(tree.node(whitespace_parent).kind(), SyntaxKind::Signature);

    let io_token = tree
        .token_at_offset(io_offset)
        .expect("IO should have a token");
    let io_parent = tree
        .token_parent(io_token)
        .expect("meaningful tokens should have parents");

    assert_eq!(tree.node(io_parent).kind(), SyntaxKind::NamePath);
    assert_ne!(whitespace_parent, io_parent);
}

#[test]
fn cst_attaches_doc_comments_to_neighboring_tokens() {
    let source = "\
-- | Top-level docs.
main = 1 -- ^ trailing docs

-- | Type docs.
data Documented =
    -- | Constructor docs.
    MkDocumented Int
";
    let parsed = parse_test_source(source);
    let tree = &parsed.cst;

    let main = token_at_text(tree, source, "main");
    assert_attached_comment_texts(
        tree,
        source,
        tree.leading_comments(main),
        &["-- | Top-level docs."],
    );

    let one = token_at_text(tree, source, "1");
    assert_attached_comment_texts(
        tree,
        source,
        tree.trailing_comments(one),
        &["-- ^ trailing docs"],
    );

    let data = token_at_text(tree, source, "data");
    assert_attached_comment_texts(
        tree,
        source,
        tree.leading_comments(data),
        &["-- | Type docs."],
    );

    let constructor = token_at_text(tree, source, "MkDocumented");
    assert_attached_comment_texts(
        tree,
        source,
        tree.leading_comments(constructor),
        &["-- | Constructor docs."],
    );
}

#[test]
fn doc_comment_text_has_one_text_owner_and_one_attachment_owner() {
    let source = "-- | Docs.\nmain = 1\n";
    let parsed = parse_test_source(source);
    let tree = &parsed.cst;
    let reconstructed = tree.reconstruct_text(source);

    assert_eq!(reconstructed.matches("-- | Docs.").count(), 1);

    let attached_count = tree
        .tokens()
        .iter()
        .enumerate()
        .flat_map(|(index, _)| {
            let id = TokenId::new(index);
            tree.leading_comments(id)
                .iter()
                .chain(tree.trailing_comments(id).iter())
        })
        .filter(|comment| tree.token(**comment).source_text(source) == "-- | Docs.")
        .count();

    assert_eq!(attached_count, 1);
}

#[test]
fn cst_does_not_attach_leading_comment_across_blank_line() {
    let source = "-- | Detached docs.\n\nmain = 1\n";
    let parsed = parse_test_source(source);
    let tree = &parsed.cst;
    let main = token_at_text(tree, source, "main");

    assert!(tree.leading_comments(main).is_empty());
}

#[test]
fn cst_round_trips_repository_solveig_sources() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("syntax crate should live under crates/");
    let mut files = Vec::new();
    collect_solveig_sources(repo, &mut files);
    files.sort();

    let session = Session::new();
    for path in files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let file = session.add_file(&path, source.clone());
        let parsed = parse_source(&session, file, &source);

        assert_eq!(
            parsed.cst.reconstruct_text(&source),
            source,
            "CST reconstruction changed {}",
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

fn assert_node_span_text(
    tree: &SyntaxTree,
    source: &str,
    kind: SyntaxKind,
    needle: &str,
    expected: &str,
) {
    let offset = source
        .find(needle)
        .expect("fixture should contain node text");
    let node = tree
        .node_at_offset(offset)
        .expect("offset should resolve to a CST node");
    assert_eq!(tree.node(node).kind(), kind);
    assert_eq!(
        source
            .get(tree.node(node).span().to_usize_range())
            .expect("CST test span should split source at UTF-8 boundaries"),
        expected
    );
}

fn token_at_text(tree: &SyntaxTree, source: &str, needle: &str) -> TokenId {
    let offset = source
        .find(needle)
        .expect("fixture should contain token text");
    tree.token_at_offset(offset)
        .expect("fixture token should resolve in the CST")
}

fn assert_attached_comment_texts(
    tree: &SyntaxTree,
    source: &str,
    comments: &[TokenId],
    expected: &[&str],
) {
    let actual = comments
        .iter()
        .map(|comment| tree.token(*comment).source_text(source))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}
