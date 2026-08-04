#![expect(
    clippy::panic,
    reason = "integration tests use panic messages to explain type fixture failures"
)]

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use solveig_driver::{ProjectSchemes, checked_project_schemes};

mod snapshot_support;

#[test]
fn checker_api_golden_types() {
    assert_scheme_fixture("checker_api");
}

#[test]
fn inferred_constraint_golden_types() {
    assert_scheme_fixture("inferred");
}

#[test]
fn cli_does_not_see_unimported_instances() {
    let case_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/types/unimported_instance");
    let output = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .arg("--project-root")
        .arg(&case_root)
        .arg("--verify-core")
        .output()
        .expect("Solveig CLI should run for unimported-instance fixture");
    let actual = normalize_text(&format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        strip_ansi(&String::from_utf8_lossy(&output.stderr))
    ));

    assert!(
        !output.status.success(),
        "unimported-instance fixture unexpectedly compiled:\n{actual}"
    );

    assert_expected_file(&case_root, "unimported_instance", &actual);
}

fn assert_scheme_fixture(name: &str) {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/types")
        .join(name);
    let schemes = checked_project_schemes(&project_root)
        .unwrap_or_else(|error| panic!("type golden fixture `{name}` should check: {error}"));
    let actual = dump_schemes(&schemes);

    assert_expected_file(&project_root, name, &actual);
}

fn assert_expected_file(case_root: &Path, label: &str, actual: &str) {
    let expected_path = case_root.join("expected.txt");
    let update = env::var("UPDATE_GOLDEN").is_ok_and(|value| value == "1");

    let expected = if update {
        fs::write(&expected_path, actual).unwrap_or_else(|error| {
            panic!(
                "failed to update type golden `{label}` at {}: {error}",
                expected_path.display()
            )
        });
        actual.to_string()
    } else {
        fs::read_to_string(&expected_path).unwrap_or_else(|error| {
            panic!(
                "failed to read type golden `{label}` at {}: {error}\n\
                 run with UPDATE_GOLDEN=1 to create or refresh it",
                expected_path.display()
            )
        })
    };

    snapshot_support::assert_fixture_snapshot(case_root, label, &expected, actual);
}

fn dump_schemes(schemes: &ProjectSchemes) -> String {
    let mut out = String::new();

    for (name, scheme) in schemes {
        out.push_str(name);
        out.push_str(" : ");
        out.push_str(scheme);
        out.push('\n');
    }

    out
}

fn normalize_text(actual: &str) -> String {
    actual.replace(env!("CARGO_MANIFEST_DIR"), "$WORKSPACE")
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
