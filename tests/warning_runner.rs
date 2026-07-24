use std::path::Path;
use std::process::Command;

#[test]
fn warning_diagnostics_do_not_fail_compile() {
    let case_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/warn/unreachable_pattern");

    let output = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .arg("--project-root")
        .arg(&case_root)
        .arg("--verify-core")
        .output()
        .unwrap_or_else(|err| {
            panic!(
                "failed to run Solveig CLI for warning case {}: {err}",
                case_root.display()
            )
        });

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));

    assert!(
        output.status.success(),
        "warning case unexpectedly failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(stdout, "1\n");
    assert!(stderr.contains("[TC0017] Warning: unreachable pattern"));
    assert!(stderr.contains("this pattern is already covered by earlier case arms"));
}

#[test]
fn orphan_instance_warning_does_not_fail_compile() {
    let case_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/warn/orphan_instance");

    let output = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .arg("--project-root")
        .arg(&case_root)
        .arg("--verify-core")
        .output()
        .unwrap_or_else(|err| {
            panic!(
                "failed to run Solveig CLI for warning case {}: {err}",
                case_root.display()
            )
        });

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));

    assert!(
        output.status.success(),
        "orphan warning case unexpectedly failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(stdout, "5\n");
    assert!(
        stderr.contains("[TC0018] Warning: orphan instance for `orphanLib.ForeignShow ForeignBox`")
    );
    assert!(stderr.contains("outside both the class and type modules"));
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
