use std::fs;
use std::path::Path;

use solveig_driver::run_project_capture;
use tempfile::TempDir;

#[test]
fn language_tour_documentation_project_compiles_and_runs() {
    let repository = repository_root();
    run_copied_project(
        &repository.join("docs/tutorial/code/language_tour"),
        "high: 12\n[\"ada@example.test\"]\n(\"answer\", 42)\n(7, \"seven\")\n[1, 2, 3]\n[(\"/health\", 1), (\"/users\", 2)]\n3-4-5 diagonal: 5\nlarge integer: Just 123456789012345678901234567890\n",
        |_| {},
    );
}

#[test]
fn file_summary_documentation_project_compiles_and_runs() {
    let repository = repository_root();
    run_copied_project(
        &repository.join("docs/tutorial/code/file_summary"),
        "readings: 4\n",
        |root| {
            assert_eq!(
                fs::read_to_string(root.join("summary.txt"))
                    .expect("file tutorial should create its report"),
                "readings: 4\n"
            );
        },
    );
}

#[test]
fn concurrent_jobs_documentation_project_compiles_and_runs() {
    let repository = repository_root();
    run_copied_project(
        &repository.join("docs/tutorial/code/concurrent_jobs"),
        "results: [1, 4, 9, 16]\noverlap: [\"invoice-101\", \"invoice-102\", \"invoice-103\"]\n",
        |_| {},
    );
}

#[test]
fn local_http_documentation_project_compiles_and_runs() {
    let repository = repository_root();
    run_copied_project(
        &repository.join("docs/tutorial/code/local_http"),
        "responses: 3\nhello from Solveig\n",
        |_| {},
    );
}

#[test]
fn tutorial_checks_documentation_project_compiles_and_runs() {
    let repository = repository_root();
    run_copied_project(
        &repository.join("docs/tutorial/code/tutorial_checks"),
        "all checks passed\n",
        |_| {},
    );
}

fn repository_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn run_copied_project(source: &Path, expected_output: &str, check_files: impl FnOnce(&Path)) {
    let temporary = TempDir::new().expect("tutorial project temp directory should be creatable");
    copy_tree(source, temporary.path());

    let output = run_project_capture(temporary.path(), "main")
        .expect("tutorial documentation project should compile and run");
    assert_eq!(
        output,
        expected_output,
        "tutorial project {}",
        source.display()
    );
    check_files(temporary.path());
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
