use std::env;
use std::fs;
use std::panic;
use std::path::{Path, PathBuf};

use solveig_driver::run_project_capture;

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

        let actual = panic::catch_unwind(|| run_project_capture(&case_root, "main"))
            .unwrap_or_else(|payload| {
                panic!(
                    "golden run case `{case_name}` panicked:\n{}",
                    panic_message(&payload)
                )
            })
            .unwrap_or_else(|err| panic!("golden run case `{case_name}` failed:\n{err}"));

        let expected_path = case_root.join("expected.txt");

        if update {
            fs::write(&expected_path, &actual).unwrap_or_else(|err| {
                panic!(
                    "failed to update expected output for `{case_name}` at {}: {err}",
                    expected_path.display()
                )
            });
            continue;
        }

        let expected = fs::read_to_string(&expected_path).unwrap_or_else(|err| {
            panic!(
                "failed to read expected output for `{case_name}` at {}: {err}\n\
                 run with UPDATE_GOLDEN=1 to create or refresh it",
                expected_path.display()
            )
        });

        assert_eq!(
            actual,
            expected,
            "golden run case `{case_name}` changed:\n{}",
            diff_preview(&expected, &actual)
        );
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

fn diff_preview(expected: &str, actual: &str) -> String {
    let first_diff = expected
        .bytes()
        .zip(actual.bytes())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.len().min(actual.len()));

    format!(
        "expected len: {}\nactual len:   {}\nfirst diff byte: {}\nexpected: {}\nactual:   {}",
        expected.len(),
        actual.len(),
        first_diff,
        preview_around(expected, first_diff),
        preview_around(actual, first_diff)
    )
}

fn preview_around(text: &str, byte_index: usize) -> String {
    let start = text[..byte_index.min(text.len())]
        .char_indices()
        .rev()
        .nth(80)
        .map(|(index, _)| index)
        .unwrap_or(0);

    let end = text[byte_index.min(text.len())..]
        .char_indices()
        .nth(160)
        .map(|(index, _)| byte_index.min(text.len()) + index)
        .unwrap_or(text.len());

    let prefix = if start > 0 { "..." } else { "" };
    let suffix = if end < text.len() { "..." } else { "" };

    format!("{prefix}{:?}{suffix}", &text[start..end])
}
