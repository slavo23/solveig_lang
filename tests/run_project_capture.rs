use std::path::Path;

use solveig_lang::run_project_capture;

#[test]
fn captures_example_project_output() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("example_project");

    let output = run_project_capture(&project_root, "main").expect("example_project should run");

    assert_eq!(output, "()");
}
