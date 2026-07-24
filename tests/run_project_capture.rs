use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use solveig_driver::{
    Compilation, CompileOptions, run_project_capture, run_project_capture_with_stdin,
};

fn project_run_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("project run lock should not be poisoned")
}

fn temp_project_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("solveig_{name}_{}_{}", process::id(), nanos))
}

fn write_temp_project(name: &str, main_source: &str) -> PathBuf {
    let project_root = temp_project_root(name);
    fs::create_dir_all(project_root.join("src")).unwrap_or_else(|err| {
        panic!(
            "failed to create temp Solveig project at {}: {err}",
            project_root.display()
        )
    });
    fs::write(project_root.join("src/main.sv"), main_source).unwrap_or_else(|err| {
        panic!(
            "failed to write temp Solveig main source at {}: {err}",
            project_root.join("src/main.sv").display()
        )
    });
    project_root
}

#[test]
fn captures_smoke_suite_output() {
    let _guard = project_run_lock();
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/smoke_suite");

    let output = run_project_capture(&project_root, "main").expect("smoke suite should run");

    assert_eq!(output, "()");
}

#[test]
fn quiz_game_example_reads_file_prompts_scores_and_writes_reports() {
    let _guard = project_run_lock();
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/quiz_game");
    let last_score = project_root.join("last-score.txt");
    let score_history = project_root.join("score-history.txt");
    let _ = fs::remove_file(&last_score);
    let _ = fs::remove_file(&score_history);

    let output = run_project_capture_with_stdin(&project_root, "main", "Oslo\n5\nblue\n")
        .expect("quiz game example should run");

    assert_eq!(
        output,
        "Solveig quiz\nCapital of Norway?\n> 2 + 2?\n> Sky color on a clear day?\n> Score: 2/3\n"
    );
    assert_eq!(
        fs::read_to_string(&last_score).expect("last-score.txt should be written"),
        "Score: 2/3\n"
    );
    assert_eq!(
        fs::read_to_string(&score_history).expect("score-history.txt should be appended"),
        "Score: 2/3\n"
    );

    let _ = fs::remove_file(last_score);
    let _ = fs::remove_file(score_history);
}

#[test]
fn io_streams_prompt_readline_and_program_args() {
    let _guard = project_run_lock();
    let project_root = write_temp_project(
        "io_streams",
        r#"main : IO ()
main = do
    printStr "name> "
    name <- readLine
    printLine name
    args <- getArgs
    mapIO_ printLine args
"#,
    );

    let mut stdin = Cursor::new("Solveig\n");
    let mut stdout = Vec::new();
    let args = vec!["one".to_string(), "two".to_string()];

    Compilation::new(&project_root)
        .run_project_with_streams(
            "main",
            CompileOptions::test_support(),
            &mut stdin,
            &mut stdout,
            &args,
        )
        .expect("stream-backed IO project should run");

    assert_eq!(
        String::from_utf8(stdout).expect("Solveig stdout should be UTF-8"),
        "name> Solveig\none\ntwo\n"
    );

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn file_io_reads_writes_and_appends_project_relative_text() {
    let _guard = project_run_lock();
    let project_root = write_temp_project(
        "file_io",
        r#"main : IO ()
main = do
    writeFile "score.txt" "seed\n"
    appendFile "score.txt" "extra\n"
    contents <- readFile "score.txt"
    printStr contents
"#,
    );

    let output = run_project_capture(&project_root, "main").expect("file IO project should run");

    assert_eq!(output, "seed\nextra\n");
    assert_eq!(
        fs::read_to_string(project_root.join("score.txt")).expect("score file should exist"),
        "seed\nextra\n"
    );

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn cli_passes_program_args_after_separator() {
    let _guard = project_run_lock();
    let project_root = write_temp_project(
        "cli_args",
        r#"main : IO ()
main = do
    args <- getArgs
    mapIO_ printLine args
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .arg("--project-root")
        .arg(&project_root)
        .arg("--")
        .arg("north")
        .arg("south")
        .output()
        .expect("solveig CLI should run");

    assert!(
        output.status.success(),
        "solveig CLI failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("Solveig stdout should be UTF-8"),
        "north\nsouth\n"
    );

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn primitive_int_division_by_zero_reports_runtime_error() {
    let _guard = project_run_lock();
    let project_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime/int_builtin_div_zero");

    let err = run_project_capture(&project_root, "main")
        .expect_err("intDiv division by zero should report a runtime error");

    assert!(
        err.contains("runtime error: intDiv division by zero"),
        "unexpected error: {err}"
    );
}

#[test]
fn primitive_int_modulo_by_zero_reports_runtime_error() {
    let _guard = project_run_lock();
    let project_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime/int_builtin_mod_zero");

    let err = run_project_capture(&project_root, "main")
        .expect_err("intMod modulo by zero should report a runtime error");

    assert!(
        err.contains("runtime error: intMod modulo by zero"),
        "unexpected error: {err}"
    );
}

#[test]
fn prelude_int_division_by_zero_reports_runtime_error() {
    let _guard = project_run_lock();
    let project_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime/int_surface_div_zero");

    let err = run_project_capture(&project_root, "main")
        .expect_err("prelude div by zero should report a runtime error");

    assert!(
        err.contains("runtime error: intDiv division by zero"),
        "unexpected error: {err}"
    );
}
