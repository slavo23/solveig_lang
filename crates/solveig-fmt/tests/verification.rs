mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use solveig_base::{FileId, Session};
use solveig_fmt::format;
use solveig_syntax::lexer::{self, Token};
use solveig_syntax::lowering::parse_source;

const FORMAT_WIDTH: usize = 100;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn generated_programs_round_trip_and_are_idempotent(source in program_strategy()) {
        let result = verify_source(Path::new("generated.sv"), &source, FORMAT_WIDTH);
        prop_assert!(
            result.is_ok(),
            "{}\n\ngenerated source:\n{source}",
            result.as_ref().err().map_or("unknown verification failure", String::as_str)
        );
    }
}

#[test]
fn repository_corpus_preserves_ast_trivia_width_and_idempotence() {
    let mut files = formatter_corpus()
        .unwrap_or_else(|error| panic!("failed to discover formatter corpus: {error}"));
    files.sort();

    assert_eq!(
        files.len(),
        184,
        "formatter corpus changed; audit the new files and update the explicit count"
    );

    for path in files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        if let Err(error) = verify_source(&path, &source, FORMAT_WIDTH) {
            panic!("{}: {error}", path.display());
        }
    }
}

fn formatter_corpus() -> std::io::Result<Vec<PathBuf>> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("ICE: formatter crate must live under the repository crates directory");
    let mut files = Vec::new();
    for relative in ["examples", "tests/golden/run", "lib/prelude", "lib/std"] {
        collect_solveig_sources(&repository.join(relative), &mut files)?;
    }
    Ok(files)
}

fn program_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(definition_body_strategy(), 1..6).prop_map(|bodies| {
        bodies
            .into_iter()
            .enumerate()
            .map(|(index, body)| format!("generated{index} = {body}\n"))
            .collect()
    })
}

fn definition_body_strategy() -> BoxedStrategy<String> {
    let expression = expression_strategy();
    prop_oneof![
        5 => expression.clone(),
        2 => (expression.clone(), expression.clone())
            .prop_map(|(value, result)| format!("do let local = {value}; {result}")),
        2 => (expression.clone(), expression)
            .prop_map(|(scrutinee, result)| format!("case {scrutinee} of _ => {result}")),
    ]
    .boxed()
}

fn expression_strategy() -> BoxedStrategy<String> {
    let leaf = prop_oneof![
        (0_i64..10_000).prop_map(|value| value.to_string()),
        proptest::sample::select(vec!["x", "y", "True", "False", "Nothing", "\"text\""])
            .prop_map(str::to_string),
    ];

    leaf.prop_recursive(4, 64, 6, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(left, right)| format!("({left} + {right})")),
            (inner.clone(), inner.clone())
                .prop_map(|(function, argument)| format!("({function}) ({argument})")),
            inner
                .clone()
                .prop_map(|body| format!("(fun parameter => {body})")),
            (inner.clone(), inner.clone())
                .prop_map(|(value, body)| format!("(let local = {value} in {body})")),
            (inner.clone(), inner.clone(), inner.clone()).prop_map(
                |(condition, then_branch, else_branch)| {
                    format!("(if {condition} then {then_branch} else {else_branch})")
                }
            ),
            (inner.clone(), inner.clone())
                .prop_map(|(first, second)| format!("[{first}, {second}]")),
            (inner.clone(), inner).prop_map(|(first, second)| format!("({first}, {second})")),
        ]
    })
    .boxed()
}

fn collect_solveig_sources(root: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_solveig_sources(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "sv") {
            files.push(path);
        }
    }
    Ok(())
}

fn verify_source(path: &Path, source: &str, width: usize) -> Result<(), String> {
    let original_session = Session::new();
    let original_file = original_session.add_file(path, source);
    let original = parse_source(&original_session, original_file, source);
    if !original_session.diagnostics().is_empty() {
        return Err(format!(
            "input did not parse cleanly: {:?}",
            original_session.diagnostics().sorted_by_file_and_span()
        ));
    }

    let formatted = format(&original.cst, source, width);
    let formatted_session = Session::new();
    let formatted_file = formatted_session.add_file(path, &formatted);
    let reparsed = parse_source(&formatted_session, formatted_file, &formatted);
    if !formatted_session.diagnostics().is_empty() {
        let diagnostics = formatted_session.diagnostics().sorted_by_file_and_span();
        let context = diagnostics
            .first()
            .and_then(|diagnostic| diagnostic.anchor_span())
            .and_then(|anchor| formatted.get(anchor.span.start as usize..))
            .and_then(|rest| rest.lines().next())
            .unwrap_or("");
        return Err(format!(
            "formatted output did not parse cleanly: {diagnostics:?}\nfirst diagnostic suffix: {context:?}\n\n{formatted}",
        ));
    }

    let before = support::normalized_ast(&original_session, &original);
    let after = support::normalized_ast(&formatted_session, &reparsed);
    if before != after {
        return Err(format!(
            "formatting changed the normalized surface AST\n\nbefore:\n{before}\n\nafter:\n{after}\n\nformatted:\n{formatted}"
        ));
    }

    let reformatted = format(&reparsed.cst, &formatted, width);
    if formatted != reformatted {
        let first_lines = formatted.lines().collect::<Vec<_>>();
        let second_lines = reformatted.lines().collect::<Vec<_>>();
        let difference = first_lines
            .iter()
            .zip(&second_lines)
            .position(|(first, second)| first != second)
            .unwrap_or(first_lines.len().min(second_lines.len()));
        return Err(format!(
            "formatting was not idempotent at line {}\nfirst: {:?}\nsecond: {:?}",
            difference + 1,
            first_lines.get(difference),
            second_lines.get(difference)
        ));
    }

    let original_comments = comments(source)?;
    let formatted_comments = comments(&formatted)?;
    if original_comments != formatted_comments {
        let changed = original_comments
            .keys()
            .chain(formatted_comments.keys())
            .filter_map(|comment| {
                let before = original_comments.get(comment).copied().unwrap_or(0);
                let after = formatted_comments.get(comment).copied().unwrap_or(0);
                (before != after).then(|| format!("{comment:?}: {before} -> {after}"))
            })
            .collect::<BTreeSet<_>>();
        return Err(format!(
            "comments were not preserved exactly once: {changed:?}"
        ));
    }

    verify_width(&formatted, width)
}

fn comments(source: &str) -> Result<BTreeMap<String, usize>, String> {
    let tokens = lexer::lex(FileId(0), source).map_err(|error| format!("lex failed: {error:?}"))?;
    let mut comments = BTreeMap::new();
    for token in tokens {
        if matches!(token.kind, Token::LineComment | Token::BlockComment) {
            *comments
                .entry(token.source_text(source).to_string())
                .or_default() += 1;
        }
    }
    Ok(comments)
}

fn verify_width(source: &str, width: usize) -> Result<(), String> {
    let tokens = lexer::lex(FileId(0), source).map_err(|error| format!("lex failed: {error:?}"))?;
    let mut unbreakable_lines = BTreeSet::new();
    for token in tokens {
        if token.is_synthetic()
            || matches!(token.kind, Token::Whitespace | Token::Newline | Token::Eof)
        {
            continue;
        }
        for (line_offset, segment) in token.source_text(source).split('\n').enumerate() {
            if segment.chars().count() > width {
                let line_offset = u32::try_from(line_offset).map_err(|error| {
                    format!("one formatter token spans too many lines: {error}")
                })?;
                unbreakable_lines.insert(token.line.saturating_add(line_offset));
            }
        }
    }

    for (line_index, line) in source.lines().enumerate() {
        let line_width = line.chars().count();
        if line_width <= width {
            continue;
        }
        let line_index = u32::try_from(line_index)
            .map_err(|error| format!("formatter output contains too many lines: {error}"))?;
        if !unbreakable_lines.contains(&line_index) {
            return Err(format!(
                "line {} has width {line_width}, exceeding {width} without one unbreakable token:\n{line}",
                line_index + 1
            ));
        }
    }
    Ok(())
}
