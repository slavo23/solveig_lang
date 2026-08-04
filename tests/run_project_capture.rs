#![expect(
    clippy::panic,
    reason = "integration tests use panic messages to explain fixture setup failures"
)]

use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;

use solveig_driver::{
    Compilation, CompileOptions, run_project_capture, run_project_capture_with_stdin,
};

mod support;

fn project_run_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("project run lock should not be poisoned")
}

fn temp_project_root(name: &str) -> support::TestRoot {
    support::temp_root("solveig_run", name)
}

fn write_temp_project(name: &str, main_source: &str) -> support::TestRoot {
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
    remove_file_if_exists(&last_score);
    remove_file_if_exists(&score_history);

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

    remove_file_if_exists(&last_score);
    remove_file_if_exists(&score_history);
}

fn remove_file_if_exists(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("failed to remove {}: {error}", path.display()),
    }
}

#[test]
fn scry_example_answers_rule_queries() {
    let _guard = project_run_lock();
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/scry");
    let stdin = fs::read_to_string(project_root.join("sample-input.txt"))
        .expect("scry sample input should be readable");

    let output = run_project_capture_with_stdin(&project_root, "main", &stdin)
        .expect("scry example should run");

    assert_eq!(
        output,
        "Scry loaded 10 facts/rules. Type :help for commands.\n\
         scry> true\n\
         scry> ?who = clara\n\
         scry> ?who = bob\n\
         ?who = faye\n\
         ?who = clara\n\
         ?who = dima\n\
         scry> ?who = alice\n\
         ?who = clara\n\
         scry> false\n\
         scry> bye\n"
    );
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

    let mut compilation = Compilation::new(&project_root);
    compilation
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

    drop(project_root);
}

#[test]
fn io_bind_chain_allocates_under_gc_stress() {
    const STEPS: usize = 10_000;

    let _guard = project_run_lock();
    let project_root = write_temp_project(
        "io_gc_stress",
        &format!(
            r#"repeat : Int -> IO ()
repeat n =
    if n == 0 then pureIO ()
    else do
        line <- readLine
        printStr (appendString line "")
        repeat (n - 1)

main : IO ()
main = repeat {STEPS}
"#
        ),
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .arg("--project-root")
        .arg(&project_root)
        .env("SOLVEIG_GC_STRESS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("solveig CLI should start for IO/GC stress test");

    let write_result = child
        .stdin
        .take()
        .expect("stress child stdin should be piped")
        .write_all("x\n".repeat(STEPS).as_bytes());

    let output = child
        .wait_with_output()
        .expect("stress child should produce an exit status");

    assert!(
        write_result.is_ok(),
        "IO/GC stress stdin write failed: {}\nstatus: {}\nstdout bytes: {}\nstderr:\n{}",
        write_result.expect_err("checked error result should contain an error"),
        output.status,
        output.stdout.len(),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        output.status.success(),
        "IO/GC stress program failed\nstatus: {}\nstdout bytes: {}\nstderr:\n{}",
        output.status,
        output.stdout.len(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("Solveig stdout should be UTF-8");
    assert_eq!(stdout.len(), STEPS);
    assert!(stdout.bytes().all(|byte| byte == b'x'));

    drop(project_root);
}

#[test]
fn million_step_io_loop_runs_on_small_native_stack() {
    const SMALL_STACK_BYTES: usize = 2 * 1024 * 1024;

    let _guard = project_run_lock();
    let project_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/run/io_million_bind_loop");

    let handle = thread::Builder::new()
        .name("solveig-io-small-stack".to_string())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(move || run_project_capture(&project_root, "main"))
        .expect("small-stack IO test thread should start");

    let output = handle
        .join()
        .expect("small-stack IO test should not panic")
        .expect("million-step IO project should run");

    assert_eq!(output, "done\n");
}

#[test]
fn repeated_checks_reuse_compiler_prelude_cache() {
    let project_root = write_temp_project("prelude_cache", "main = ()\n");
    let mut compilation = Compilation::new(&project_root);

    assert!(
        compilation
            .check(true)
            .expect("first check should run")
            .success(),
        "first check should succeed"
    );
    let first_prelude_files = compilation
        .source_map()
        .files()
        .iter()
        .filter(|file| {
            file.path
                .to_string_lossy()
                .starts_with("<compiler-prelude>/")
        })
        .count();

    assert!(
        compilation
            .check(true)
            .expect("second check should run")
            .success(),
        "second check should succeed"
    );
    let second_prelude_files = compilation
        .source_map()
        .files()
        .iter()
        .filter(|file| {
            file.path
                .to_string_lossy()
                .starts_with("<compiler-prelude>/")
        })
        .count();

    assert!(first_prelude_files > 0, "Prelude files should be loaded");
    assert_eq!(
        first_prelude_files, second_prelude_files,
        "cached checks should not add another virtual Prelude file set"
    );

    drop(project_root);
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

    drop(project_root);
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

    drop(project_root);
}

#[test]
fn primitive_int_division_by_zero_reports_runtime_error() {
    let _guard = project_run_lock();
    let project_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime/int_builtin_div_zero");

    let err = run_project_capture(&project_root, "main")
        .expect_err("intDiv division by zero should report a runtime error");

    assert!(
        err.to_string()
            .contains("runtime error: intDiv division by zero"),
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
        err.to_string()
            .contains("runtime error: intMod modulo by zero"),
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
        err.to_string()
            .contains("runtime error: intDiv division by zero"),
        "unexpected error: {err}"
    );
}

#[test]
fn cyclic_globals_report_runtime_error() {
    let _guard = project_run_lock();
    let project_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime/cyclic_global");

    let err = run_project_capture(&project_root, "main").expect_err("cyclic globals should report");

    assert!(
        err.to_string()
            .contains("runtime error: cyclic global `main.left`"),
        "unexpected error: {err}"
    );
}
