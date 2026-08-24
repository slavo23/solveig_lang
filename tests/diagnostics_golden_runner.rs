#![expect(
    clippy::panic,
    reason = "golden runner panics with fixture context when test setup fails"
)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use solveig_driver::Compilation;
use solveig_report::render_diagnostics_no_color;

use crate::fixture_shards;
use crate::snapshot_support;

#[test]
fn golden_diagnostic_cases_shard_1_of_4() {
    run_diagnostic_shard(0);
}

#[test]
fn golden_diagnostic_cases_shard_2_of_4() {
    run_diagnostic_shard(1);
}

#[test]
fn golden_diagnostic_cases_shard_3_of_4() {
    run_diagnostic_shard(2);
}

#[test]
fn golden_diagnostic_cases_shard_4_of_4() {
    run_diagnostic_shard(3);
}

fn run_diagnostic_shard(shard_index: usize) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/diag");
    let update = env::var("UPDATE_GOLDEN").is_ok_and(|value| value == "1");

    let mut cases = golden_cases(&root);
    cases.sort();
    let cases = fixture_shards::select_fixture_shard(cases, shard_index);

    assert!(
        !cases.is_empty(),
        "golden diagnostic shard {} is empty",
        shard_index + 1
    );

    for case_root in cases {
        let case_name = case_name(&case_root);
        let actual = run_cli_case(&case_root, case_name);

        assert!(
            !actual.contains("panicked at") && !actual.contains("thread 'main' panicked"),
            "golden diagnostic case `{case_name}` panicked instead of reporting a diagnostic:\n\
             {actual}"
        );

        let expected_path = case_root.join("expected.txt");

        let expected = if update {
            fs::write(&expected_path, &actual).unwrap_or_else(|err| {
                panic!(
                    "failed to update expected diagnostics for `{case_name}` at {}: {err}",
                    expected_path.display()
                )
            });
            actual.clone()
        } else {
            fs::read_to_string(&expected_path).unwrap_or_else(|err| {
                panic!(
                    "failed to read expected diagnostics for `{case_name}` at {}: {err}\n\
                     run with UPDATE_GOLDEN=1 to create or refresh it",
                    expected_path.display()
                )
            })
        };
        let expected = trim_line_end_whitespace(&expected);

        snapshot_support::assert_fixture_snapshot(&case_root, case_name, &expected, &actual);
    }
}

fn run_cli_case(case_root: &Path, case_name: &str) -> String {
    let mut compilation = Compilation::new(case_root);
    let result = match compilation.check(true) {
        Ok(result) => result,
        Err(err) => return normalize_text(&format!("{err}\n")),
    };

    let mut actual = render_diagnostics_no_color(result.diagnostics(), result.source_map())
        .unwrap_or_else(|err| panic!("failed to render diagnostics for `{case_name}`: {err}"));
    if let Some(message) = result.failure_message() {
        actual.push_str(message);
        actual.push('\n');
    }

    assert!(
        !result.success(),
        "golden diagnostic case `{case_name}` unexpectedly succeeded:\n{actual}"
    );

    normalize_text(&actual)
}

fn normalize_text(actual: &str) -> String {
    let normalized_paths = actual.replace(env!("CARGO_MANIFEST_DIR"), "$WORKSPACE");
    let normalized_paths = trim_line_end_whitespace(&normalized_paths);

    normalize_cyclic_imports(&normalized_paths)
}

fn trim_line_end_whitespace(input: &str) -> String {
    let mut out = input
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");

    if input.ends_with('\n') {
        out.push('\n');
    }

    out
}

fn normalize_cyclic_imports(input: &str) -> String {
    let mut out = input
        .lines()
        .map(|line| {
            let Some(cycle) = line.strip_prefix("cyclic module imports: ") else {
                return line.to_string();
            };

            let mut names = cycle.split(" -> ").collect::<Vec<_>>();
            names.sort_unstable();

            format!("cyclic module imports: {}", names.join(" -> "))
        })
        .collect::<Vec<_>>()
        .join("\n");

    if input.ends_with('\n') {
        out.push('\n');
    }

    out
}

fn golden_cases(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .unwrap_or_else(|err| {
            panic!(
                "failed to read golden diagnostic root {}: {err}",
                root.display()
            )
        })
        .map(|entry| {
            entry
                .unwrap_or_else(|err| {
                    panic!("failed to read golden diagnostic directory entry: {err}")
                })
                .path()
        })
        .filter(|path| path.is_dir() && !path.join("ignore.txt").exists())
        .collect()
}

fn case_name(case_root: &Path) -> &str {
    case_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<invalid-case-name>")
}
