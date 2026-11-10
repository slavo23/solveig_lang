use std::fs;
use std::path::Path;

use solveig_driver::run_project_capture;
use tempfile::TempDir;

#[test]
fn log_audit_tutorial_application_compiles_and_runs() {
    let repository = repository_root();
    let output = run_copied_project(&repository.join("examples/log_audit"));

    assert_eq!(
        output,
        "Solveig log audit\n\
         files analyzed: 4\n\
         files missing: 1\n\
         events: 14\n\
         info: 8\n\
         warnings: 3\n\
         errors: 3\n"
    );
}

#[test]
fn release_radar_tutorial_application_compiles_and_runs_hermetically() {
    let repository = repository_root();
    let output = run_copied_project(&repository.join("examples/release_radar"));

    assert!(
        output.starts_with("release radar: deterministic local concurrency comparison\n"),
        "unexpected Release Radar prelude:\n{output}"
    );
    assert!(
        output.contains("local comparison: sequential "),
        "Release Radar should report its controlled comparison:\n{output}"
    );
    assert!(
        output.contains("checked 40 repositories with a bounded 4-worker pool\n"),
        "Release Radar should report its bounded workload:\n{output}"
    );
    assert_eq!(
        output.matches("[start] ").count(),
        40,
        "every configured repository should start exactly once"
    );
    assert_eq!(
        output.matches("[done ").count(),
        40,
        "every configured repository should complete exactly once"
    );
}

fn repository_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn run_copied_project(source: &Path) -> String {
    let temporary = TempDir::new().expect("tutorial project temp directory should be creatable");
    copy_tree(source, temporary.path());

    run_project_capture(temporary.path(), "main")
        .expect("tutorial application should compile and run")
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("tutorial destination should be creatable");
    for entry in fs::read_dir(source).expect("tutorial project should be readable") {
        let entry = entry.expect("tutorial directory entry should be readable");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path)
                .expect("tutorial project file should be copyable");
        }
    }
}
