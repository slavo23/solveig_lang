//! Driver overlay tests for editor-backed source text.
//! These keep the virtual file system behavior exercised through the public Compilation API.

use std::fs;

use solveig_driver::Compilation;

mod support;

fn temp_project_root(name: &str) -> support::TestRoot {
    support::temp_root("solveig_overlay", name)
}

fn write_project(name: &str, main_source: &str) -> support::TestRoot {
    let project_root = temp_project_root(name);
    fs::create_dir_all(project_root.join("src"))
        .expect("overlay test project directory should be creatable");
    fs::write(project_root.join("src/main.sv"), main_source)
        .expect("overlay test main file should be writable");
    project_root
}

#[test]
fn module_existing_only_as_overlay_is_importable() {
    let project_root = write_project(
        "importable",
        r#"import scratch(answer)

main : Int
main = answer + 1
"#,
    );
    let scratch_path = project_root.join("src/scratch.sv");
    let mut compilation = Compilation::new(&project_root);
    compilation.set_overlay(
        &scratch_path,
        r#"answer : Int
answer = 41
"#,
    );

    assert!(
        !scratch_path.exists(),
        "overlay import fixture should not exist on disk"
    );
    let output = compilation
        .run_project_capture("main")
        .expect("overlay-only imported module should check and run");

    assert_eq!(output, "42");

    drop(project_root);
}

#[test]
fn clearing_overlay_reverts_to_disk_source() {
    let project_root = write_project(
        "clear",
        r#"main : Int
main = 7
"#,
    );
    let mut compilation = Compilation::new(&project_root);

    compilation.set_overlay(
        "src/main.sv",
        r#"main : Int
main = 41
"#,
    );
    assert_eq!(
        compilation
            .run_project_capture("main")
            .expect("overlaid main should run"),
        "41"
    );

    compilation.clear_overlay("src/main.sv");
    assert_eq!(
        compilation
            .run_project_capture("main")
            .expect("disk main should run after clearing overlay"),
        "7"
    );

    drop(project_root);
}

#[test]
fn overlay_supplies_entry_file_with_no_disk_file() {
    let project_root = temp_project_root("entry");
    fs::create_dir_all(project_root.join("src"))
        .expect("overlay-only entry project directory should be creatable");
    let main_path = project_root.join("src/main.sv");
    let mut compilation = Compilation::new(&project_root);
    compilation.set_overlay(
        "src/main.sv",
        r#"main : Int
main = 99
"#,
    );

    assert!(
        !main_path.exists(),
        "overlay entry fixture should not exist on disk"
    );
    let output = compilation
        .run_project_capture("main")
        .expect("overlay-only entry should check and run");

    assert_eq!(output, "99");

    drop(project_root);
}
