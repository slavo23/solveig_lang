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

use crate::support;

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
        r#"import std.env(getArgs)

main : Task ()
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
fn byte_file_and_handle_io_preserves_invalid_utf8() {
    let _guard = project_run_lock();
    let project_root = write_temp_project(
        "binary_io",
        r#"import std.bytes(*)
import std.io(*)

readBytes : Handle -> Task Bytes;
readBytes input = hGetBytes input 64;

writeBytes : Bytes -> Handle -> Task ();
writeBytes bytes output = hPutBytes output bytes;

describeUtf8 : Either Utf8Error String -> Task ();
describeUtf8 result =
    case result of
        Left (InvalidUtf8 offset) => printLine "invalid"
        | Right _ => printLine "wrong-success";

main : Task ();
main = do
    whole <- readFileBytes "invalid.bin"
    handleWrite <- withFile "handle-copy.bin" WriteMode (writeBytes whole)
    throughHandle <- withFile "invalid.bin" ReadMode readBytes
    writeFileBytes "whole-copy.bin" throughHandle
    copied <- readFileBytes "whole-copy.bin"
    printLine (show (bytesLength copied))
    describeUtf8 (bytesToString copied);
"#,
    );
    let bytes = [0, 0xff, 0x80, b'A', b'\n'];
    fs::write(project_root.join("invalid.bin"), bytes)
        .expect("invalid UTF-8 fixture should be writable");

    let output = run_project_capture(&project_root, "main")
        .expect("binary IO project should run without decoding its input");

    assert_eq!(output, "5\ninvalid\n");
    assert_eq!(
        fs::read(project_root.join("handle-copy.bin"))
            .expect("handle byte copy should be readable"),
        bytes
    );
    assert_eq!(
        fs::read(project_root.join("whole-copy.bin"))
            .expect("whole-file byte copy should be readable"),
        bytes
    );
}

#[test]
fn io_bind_chain_allocates_under_gc_stress() {
    const STEPS: usize = 10_000;

    let _guard = project_run_lock();
    let project_root = write_temp_project(
        "io_gc_stress",
        &format!(
            r#"repeat : Int -> Task ()
repeat n =
    if n == 0 then pureIO ()
    else do
        line <- readLine
        printStr (appendString line "")
        repeat (n - 1)

main : Task ()
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
fn repeated_checks_reuse_implicit_package_cache() {
    let project_root = write_temp_project("prelude_cache", "main = ()\n");
    let mut compilation = Compilation::new(&project_root);
    let prelude_root = solveig_driver::compiler_library_location()
        .expect("compiler library should resolve")
        .package_source_root("prelude")
        .into_std_path_buf();

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
        .filter(|file| file.path.starts_with(&prelude_root))
        .count();

    let reused_implicit_package = {
        let result = compilation.check(true).expect("second check should run");
        assert!(result.success(), "second check should succeed");
        result
            .reused_modules()
            .iter()
            .any(|module| module == "prelude.list")
    };
    let second_prelude_files = compilation
        .source_map()
        .files()
        .iter()
        .filter(|file| file.path.starts_with(&prelude_root))
        .count();
    assert!(
        reused_implicit_package,
        "unchanged implicit package modules should be reused"
    );

    assert!(first_prelude_files > 0, "Prelude files should be loaded");
    assert_eq!(
        first_prelude_files, second_prelude_files,
        "cached checks should not add another Prelude file set"
    );

    drop(project_root);
}

#[test]
fn file_io_reads_writes_and_appends_project_relative_text() {
    let _guard = project_run_lock();
    let project_root = write_temp_project(
        "file_io",
        r#"import std.io(*)

main : Task ()
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
        r#"import std.env(getArgs)

main : Task ()
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
fn cli_exit_flushes_an_open_file_and_propagates_its_status() {
    let _guard = project_run_lock();
    let project_root = write_temp_project(
        "cli_exit_flush",
        r#"import std.io(*)
import std.process(exitCodeFromInt, exitWith)

main : Task ()
main = do
    output <- openFile "buffered.txt" WriteMode
    hSetBuffering output (BlockBuffering (Just 4096))
    hPutStr output "flushed during exit"
    exitWith (exitCodeFromInt 37)
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .arg("--project-root")
        .arg(&project_root)
        .output()
        .expect("Solveig exit process should run");

    assert_eq!(output.status.code(), Some(37));
    assert!(output.stderr.is_empty());
    assert_eq!(
        fs::read_to_string(project_root.join("buffered.txt"))
            .expect("open output handle should be flushed during exit"),
        "flushed during exit"
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
            .contains("runtime error: uncaught defect: DivisionByZero"),
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
            .contains("runtime error: uncaught defect: DivisionByZero"),
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
            .contains("runtime error: uncaught defect: DivisionByZero"),
        "unexpected error: {err}"
    );
}

#[test]
fn catch_defect_recovers_from_managed_stack_exhaustion() {
    let _guard = project_run_lock();
    let project_root = write_temp_project(
        "stack_defect",
        r#"import std.task(catchDefect)

grow : Int -> Int
grow n =
    if n == 0 then 0
    else 1 + grow (n - 1)

stackAction : Task ()
stackAction =
    bindIO (pureIO ()) (fun ignored =>
        bindIO (pureIO (grow 10000)) (fun value => pureIO ()))

report : Defect -> Task ()
report defect =
    case defect of
        StackExhausted => printLine "stack-caught"
        | Error message => printLine "unexpected-error"
        | DivisionByZero => printLine "unexpected-division"
        | HeapExhausted => printLine "unexpected-heap"

main : Task ()
main = catchDefect stackAction report
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .arg("--project-root")
        .arg(&project_root)
        .env("SOLVEIG_STACK_LIMIT", "128")
        .output()
        .expect("Solveig stack-exhaustion process should run");

    assert!(
        output.status.success(),
        "stack exhaustion was not caught: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("Solveig stdout should be UTF-8"),
        "stack-caught\n"
    );
}

#[test]
fn catch_defect_recovers_from_managed_heap_exhaustion() {
    let _guard = project_run_lock();
    let project_root = write_temp_project(
        "heap_defect",
        r#"import std.task(catchDefect)

build : Int -> [Int] -> [Int]
build n values =
    if n == 0 then values
    else build (n - 1) (n :: values)

heapAction : Task ()
heapAction =
    bindIO (pureIO ()) (fun ignored =>
        bindIO (pureIO (build 10000 [])) (fun values => pureIO ()))

report : Defect -> Task ()
report defect =
    case defect of
        HeapExhausted => printLine "heap-caught"
        | Error message => printLine "unexpected-error"
        | DivisionByZero => printLine "unexpected-division"
        | StackExhausted => printLine "unexpected-stack"

main : Task ()
main = catchDefect heapAction report
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .arg("--project-root")
        .arg(&project_root)
        .env("SOLVEIG_HEAP_LIMIT", "4096")
        .output()
        .expect("Solveig heap-exhaustion process should run");

    assert!(
        output.status.success(),
        "heap exhaustion was not caught: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("Solveig stdout should be UTF-8"),
        "heap-caught\n"
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
