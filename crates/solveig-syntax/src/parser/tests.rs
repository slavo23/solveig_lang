use super::*;

use std::fs;
use std::path::{Path, PathBuf};

fn parse_test_events(source: &str) -> (Vec<PosToken>, Vec<Event>) {
    let parsed = parse_source_events(FileId(0), source).expect("test source should lex");
    (parsed.tokens, parsed.events)
}

fn event_balance(events: &[Event]) -> isize {
    events.iter().fold(0isize, |depth, event| match event {
        Event::StartNode(_) => depth + 1,
        Event::FinishNode => depth - 1,
        Event::Token(_) | Event::Error(_) => depth,
    })
}

fn error_event_is_inside_error_node(events: &[Event]) -> bool {
    let mut stack = Vec::new();

    for event in events {
        match event {
            Event::StartNode(kind) => stack.push(*kind),
            Event::FinishNode => {
                stack.pop();
            }
            Event::Error(diagnostic) if diagnostic.code == "PAR0001" => {
                return stack.last().copied() == Some(SyntaxKind::Error);
            }
            Event::Token(_) | Event::Error(_) => {}
        }
    }

    false
}

#[test]
fn parser_emits_balanced_events_over_full_token_stream() {
    let source = "main : IO ()\nmain = do\n    printLine \"ok\"\n";
    let (tokens, events) = parse_test_events(source);

    assert_eq!(event_balance(&events), 0);
    assert!(matches!(
        events.first(),
        Some(Event::StartNode(SyntaxKind::Module))
    ));
    assert!(matches!(events.last(), Some(Event::FinishNode)));
    assert!(events.contains(&Event::StartNode(SyntaxKind::Signature)));
    assert!(events.contains(&Event::StartNode(SyntaxKind::FunctionDef)));
    assert!(events.contains(&Event::StartNode(SyntaxKind::DoExpr)));

    let token_events = events
        .iter()
        .filter_map(|event| match event {
            Event::Token(index) => Some(*index),
            Event::StartNode(_) | Event::FinishNode | Event::Error(_) => None,
        })
        .collect::<Vec<_>>();
    let all_token_indices = (0..tokens.len()).collect::<Vec<_>>();
    assert_eq!(token_events, all_token_indices);
}

#[test]
fn parser_emits_error_event_and_node_during_recovery() {
    let (_, events) = parse_test_events("bad :\nmain = ()\n");

    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Error(diagnostic)
            if diagnostic.code == "PAR0001"))
    );
    assert!(events.contains(&Event::StartNode(SyntaxKind::Error)));
    assert!(error_event_is_inside_error_node(&events));
}

#[test]
fn parser_events_cover_every_token_for_repository_sources() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("syntax crate should live under crates/");
    let mut files = Vec::new();
    collect_solveig_sources(repo, &mut files);
    files.sort();

    for path in files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let parsed = parse_source_events(FileId(0), &source)
            .unwrap_or_else(|err| panic!("failed to parse events for {}: {err:?}", path.display()));
        let token_events = parsed
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Token(index) => Some(*index),
                Event::StartNode(_) | Event::FinishNode | Event::Error(_) => None,
            })
            .collect::<Vec<_>>();
        let all_token_indices = (0..parsed.tokens.len()).collect::<Vec<_>>();

        assert_eq!(
            token_events,
            all_token_indices,
            "parser event stream skipped or duplicated tokens for {}",
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
