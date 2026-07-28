//! Driver overlay tests for editor-backed source text.
//! These keep the virtual file system behavior exercised through the public Compilation API.

use std::fs;
use std::path::PathBuf;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use solveig_driver::Compilation;

fn temp_project_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "solveig_overlay_{name}_{}_{}",
        process::id(),
        nanos
    ))
}

fn write_project(name: &str, main_source: &str) -> PathBuf {
    let project_root = temp_project_root(name);
    fs::create_dir_all(project_root.join("src")).unwrap_or_else(|err| {
        panic!(
            "failed to create overlay test project at {}: {err}",
            project_root.display()
        )
    });
    fs::write(project_root.join("src/main.sv"), main_source).unwrap_or_else(|err| {
        panic!(
            "failed to write overlay test main file at {}: {err}",
            project_root.join("src/main.sv").display()
        )
    });
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

    let _ = fs::remove_dir_all(project_root);
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

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn overlay_supplies_entry_file_with_no_disk_file() {
    let project_root = temp_project_root("entry");
    fs::create_dir_all(project_root.join("src")).unwrap_or_else(|err| {
        panic!(
            "failed to create overlay-only entry project at {}: {err}",
            project_root.display()
        )
    });
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

    let _ = fs::remove_dir_all(project_root);
}
