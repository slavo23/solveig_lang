#![expect(
    clippy::panic,
    reason = "integration tests use panic messages to explain warning fixture failures"
)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::snapshot_support;

#[test]
fn warning_golden_cases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/warn");
    let update = env::var("UPDATE_GOLDEN").is_ok_and(|value| value == "1");

    let mut cases = golden_cases(&root);
    cases.sort();

    assert!(
        !cases.is_empty(),
        "no warning golden cases found under {}",
        root.display()
    );

    for case_root in cases {
        let case_name = case_name(&case_root);
        let actual = run_warning_case(&case_root, case_name);
        let expected_path = case_root.join("expected.txt");

        let expected = if update {
            fs::write(&expected_path, &actual).unwrap_or_else(|error| {
                panic!(
                    "failed to update warning golden for `{case_name}` at {}: {error}",
                    expected_path.display()
                )
            });
            actual.clone()
        } else {
            fs::read_to_string(&expected_path).unwrap_or_else(|error| {
                panic!(
                    "failed to read warning golden for `{case_name}` at {}: {error}\n\
                     run with UPDATE_GOLDEN=1 to create or refresh it",
                    expected_path.display()
                )
            })
        };

        snapshot_support::assert_fixture_snapshot(&case_root, case_name, &expected, &actual);
    }
}

fn run_warning_case(case_root: &Path, case_name: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .arg("--project-root")
        .arg(case_root)
        .arg("--verify-core")
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "failed to run Solveig CLI for warning case `{case_name}` at {}: {error}",
                case_root.display()
            )
        });

    let actual = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        strip_ansi(&String::from_utf8_lossy(&output.stderr))
    );

    assert!(
        output.status.success(),
        "warning case `{case_name}` unexpectedly failed:\n{actual}"
    );

    normalize_text(&actual)
}

fn normalize_text(actual: &str) -> String {
    actual.replace(env!("CARGO_MANIFEST_DIR"), "$WORKSPACE")
}

fn golden_cases(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .unwrap_or_else(|error| {
            panic!(
                "failed to read warning golden root {}: {error}",
                root.display()
            )
        })
        .map(|entry| {
            entry
                .unwrap_or_else(|error| {
                    panic!("failed to read warning golden directory entry: {error}")
                })
                .path()
        })
        .filter(|path| path.join("src/main.sv").is_file())
        .collect()
}

fn case_name(case_root: &Path) -> &str {
    case_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<invalid-case-name>")
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();

            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }

            continue;
        }

        out.push(ch);
    }

    out
}
