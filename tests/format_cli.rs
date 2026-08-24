use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::support::{TestRoot, temp_root};

const MANIFEST: &str = r#"[package]
name = "app"
version = "0.1.0"

[language]
entry = "main"
"#;

fn package(name: &str) -> TestRoot {
    let root = temp_root("solveig_fmt", name);
    fs::create_dir_all(root.join("src")).expect("formatter fixture source should be creatable");
    fs::write(root.join("solveig.toml"), MANIFEST)
        .expect("formatter fixture manifest should be writable");
    root
}

fn solveig() -> Command {
    Command::new(env!("CARGO_BIN_EXE_solveig"))
}

#[test]
fn fmt_stdin_formats_without_a_project() {
    let mut child = solveig()
        .args(["fmt", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("formatter command should start");
    child
        .stdin
        .as_mut()
        .expect("formatter stdin should be piped")
        .write_all(b"main=let x=1 in x+2")
        .expect("formatter input should be writable");

    let output = child
        .wait_with_output()
        .expect("formatter command should finish");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("output should be UTF-8"),
        "main = let x = 1 in x + 2\n"
    );
}

#[test]
fn fmt_formats_an_explicit_standalone_file() {
    let root = temp_root("solveig_fmt", "standalone");
    let source = root.join("scratch.sv");
    fs::write(&source, "answer=40+2").expect("standalone source should be writable");

    let output = solveig()
        .arg("fmt")
        .arg(&source)
        .output()
        .expect("formatter command should run");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(source).expect("standalone source should remain readable"),
        "answer = 40 + 2\n"
    );
}

#[test]
fn fmt_check_prints_a_diff_and_never_writes() {
    let root = package("check");
    let source_path = root.join("src/main.sv");
    let source = "main=let x=1 in x+2";
    fs::write(&source_path, source).expect("formatter fixture should be writable");

    let output = solveig()
        .current_dir(&root)
        .args(["fmt", "--check", "src/main.sv"])
        .output()
        .expect("formatter check should run");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).expect("diff should be UTF-8");
    assert!(stdout.starts_with("--- "));
    assert!(stdout.contains("src/main.sv\n+++ "));
    assert!(stdout.contains("src/main.sv (formatted)\n@@ "));
    assert_eq!(
        fs::read_to_string(source_path).expect("fixture should remain readable"),
        source
    );

    fs::write(root.join("src/main.sv"), "main = let x = 1 in x + 2\n")
        .expect("clean formatter fixture should be writable");
    let clean = solveig()
        .current_dir(&root)
        .args(["fmt", "--check", "src/main.sv"])
        .output()
        .expect("clean formatter check should run");

    assert!(clean.status.success());
    assert!(clean.stdout.is_empty());
}

#[test]
fn fmt_writes_every_selected_local_source() {
    let root = package("write");
    let main = root.join("src/main.sv");
    let library = root.join("src/library.sv");
    fs::write(&main, "main=1").expect("main fixture should be writable");
    fs::write(&library, "answer=40+2").expect("library fixture should be writable");

    let output = solveig()
        .current_dir(&root)
        .arg("fmt")
        .output()
        .expect("formatter command should run");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(main).expect("main should be readable"),
        "main = 1\n"
    );
    assert_eq!(
        fs::read_to_string(library).expect("library should be readable"),
        "answer = 40 + 2\n"
    );
}

#[test]
fn fmt_parse_error_leaves_every_selected_file_untouched() {
    let root = package("parse_error");
    let main = root.join("src/main.sv");
    let broken = root.join("src/z_broken.sv");
    let main_source = "main=1";
    let broken_source = "broken = let in";
    fs::write(&main, main_source).expect("main fixture should be writable");
    fs::write(&broken, broken_source).expect("broken fixture should be writable");

    let output = solveig()
        .current_dir(&root)
        .args(["fmt", "src"])
        .output()
        .expect("formatter command should run");

    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(main).expect("main should be readable"),
        main_source
    );
    assert_eq!(
        fs::read_to_string(broken).expect("broken should be readable"),
        broken_source
    );
}

#[test]
fn fmt_refuses_dependency_sources() {
    let workspace = temp_root("solveig_fmt", "dependency");
    let app = workspace.join("app");
    let dependency = workspace.join("library");
    fs::create_dir_all(app.join("src")).expect("app source should be creatable");
    fs::create_dir_all(dependency.join("src")).expect("dependency source should be creatable");
    fs::write(
        app.join("solveig.toml"),
        MANIFEST.replace(
            "[language]",
            "[dependencies]\nlibrary = { path = \"../library\" }\n\n[language]",
        ),
    )
    .expect("app manifest should be writable");
    fs::write(
        dependency.join("solveig.toml"),
        MANIFEST.replace("name = \"app\"", "name = \"library\""),
    )
    .expect("dependency manifest should be writable");
    fs::write(app.join("src/main.sv"), "main = 0\n").expect("app source should be writable");
    let dependency_source = dependency.join("src/main.sv");
    let original = "value=40+2";
    fs::write(&dependency_source, original).expect("dependency source should be writable");

    let relative_dependency = Path::new("../library/src/main.sv");
    let output = solveig()
        .current_dir(&app)
        .arg("fmt")
        .arg(relative_dependency)
        .output()
        .expect("formatter command should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot edit dependency source"),
        "unexpected formatter error: {stderr}"
    );
    assert_eq!(
        fs::read_to_string(dependency_source).expect("dependency should remain readable"),
        original
    );
}
