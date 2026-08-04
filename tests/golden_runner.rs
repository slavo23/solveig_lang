#![expect(
    clippy::panic,
    reason = "integration tests use panic messages to explain golden fixture failures"
)]

use std::env;
use std::fs;
use std::panic;
use std::path::{Path, PathBuf};

use solveig_driver::run_project_capture_with_stdin_and_args;

mod snapshot_support;

#[test]
fn golden_run_cases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/run");
    let update = env::var("UPDATE_GOLDEN").is_ok_and(|value| value == "1");

    let mut cases = golden_cases(&root);
    cases.sort();

    assert!(
        !cases.is_empty(),
        "no golden run cases found under {}",
        root.display()
    );

    for case_root in cases {
        let case_name = case_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<invalid-case-name>");

        let stdin = case_stdin(case_name, &case_root);
        let args = case_args(case_name, &case_root);
        let actual = panic::catch_unwind(|| {
            run_project_capture_with_stdin_and_args(&case_root, "main", &stdin, &args)
        })
        .unwrap_or_else(|payload| {
            panic!(
                "golden run case `{case_name}` panicked:\n{}",
                panic_message(&payload)
            )
        })
        .unwrap_or_else(|err| panic!("golden run case `{case_name}` failed:\n{err}"));

        let expected_path = case_root.join("expected.txt");

        let expected = if update {
            fs::write(&expected_path, &actual).unwrap_or_else(|err| {
                panic!(
                    "failed to update expected output for `{case_name}` at {}: {err}",
                    expected_path.display()
                )
            });
            actual.clone()
        } else {
            fs::read_to_string(&expected_path).unwrap_or_else(|err| {
                panic!(
                    "failed to read expected output for `{case_name}` at {}: {err}\n\
                     run with UPDATE_GOLDEN=1 to create or refresh it",
                    expected_path.display()
                )
            })
        };

        snapshot_support::assert_fixture_snapshot(&case_root, case_name, &expected, &actual);
    }
}

fn case_stdin(case_name: &str, case_root: &Path) -> String {
    let stdin_path = case_root.join("stdin.txt");
    match fs::read_to_string(&stdin_path) {
        Ok(stdin) => stdin,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => panic!(
            "failed to read stdin for `{case_name}` at {}: {err}",
            stdin_path.display()
        ),
    }
}

fn case_args(case_name: &str, case_root: &Path) -> Vec<String> {
    let args_path = case_root.join("args.txt");
    match fs::read_to_string(&args_path) {
        Ok(args) => args.lines().map(|arg| arg.to_string()).collect(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => panic!(
            "failed to read args for `{case_name}` at {}: {err}",
            args_path.display()
        ),
    }
}

fn golden_cases(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .unwrap_or_else(|err| panic!("failed to read golden run root {}: {err}", root.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("failed to read golden run directory entry: {err}"))
                .path()
        })
        .filter(|path| path.join("src/main.sv").is_file())
        .collect()
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}
