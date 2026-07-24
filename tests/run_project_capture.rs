use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};

use solveig_driver::run_project_capture;

fn project_run_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("project run lock should not be poisoned")
}

#[test]
fn captures_example_project_output() {
    let _guard = project_run_lock();
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("example_project");

    let output = run_project_capture(&project_root, "main").expect("example_project should run");

    assert_eq!(output, "()");
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
