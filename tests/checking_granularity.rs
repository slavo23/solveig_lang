//! Driver tests for editor-oriented check granularity and single-file entry points.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use solveig_base::Severity;
use solveig_driver::{CheckResult, Compilation};

fn temp_project_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "solveig_checking_{name}_{}_{}",
        process::id(),
        nanos
    ))
}

fn write_source(path: &Path, source: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|err| {
            panic!(
                "failed to create source directory {}: {err}",
                parent.display()
            )
        });
    }
    fs::write(path, source)
        .unwrap_or_else(|err| panic!("failed to write Solveig source {}: {err}", path.display()));
}

fn write_granularity_project(name: &str) -> PathBuf {
    let project_root = temp_project_root(name);
    write_source(
        &project_root.join("src/main.sv"),
        r#"import a(valueA)
import b(valueB)

main : Int
main = valueA + valueB
"#,
    );
    write_source(
        &project_root.join("src/a.sv"),
        r#"import c(valueC)

valueA : Int
valueA = valueC + 1
"#,
    );
    write_source(
        &project_root.join("src/b.sv"),
        r#"valueB : Int
valueB = 2
"#,
    );
    write_source(
        &project_root.join("src/c.sv"),
        r#"valueC : Int
valueC = 3
"#,
    );
    project_root
}

fn module_set(modules: &[String]) -> BTreeSet<String> {
    modules
        .iter()
        .filter(|module| !module.starts_with("prelude."))
        .cloned()
        .collect()
}

fn expected_modules<const N: usize>(modules: [&str; N]) -> BTreeSet<String> {
    modules.into_iter().map(str::to_string).collect()
}

fn primary_error_paths(result: &CheckResult) -> BTreeSet<PathBuf> {
    result
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .filter_map(|diagnostic| diagnostic.anchor_span())
        .filter_map(|span| result.source_map().files().get(span.file.0 as usize))
        .map(|file| file.path.clone())
        .collect()
}

fn assert_path_set_contains_file(paths: &BTreeSet<PathBuf>, file_name: &str) {
    assert!(
        paths
            .iter()
            .any(|path| path.file_name().and_then(|name| name.to_str()) == Some(file_name)),
        "expected diagnostics for {file_name}, got {paths:#?}"
    );
}

#[test]
fn leaf_edit_rechecks_only_that_module_and_root_edit_rechecks_dependents() {
    let project_root = write_granularity_project("dirty_set");
    let mut compilation = Compilation::new(&project_root);

    let initial_fresh = {
        let result = compilation.check(true).expect("initial check should run");
        assert!(result.success(), "initial check should succeed");
        module_set(result.freshly_checked_modules())
    };
    assert!(initial_fresh.contains("main"));
    assert!(initial_fresh.contains("a"));
    assert!(initial_fresh.contains("b"));
    assert!(initial_fresh.contains("c"));

    compilation.set_overlay(
        "src/main.sv",
        r#"import a(valueA)
import b(valueB)

main : Int
main = valueA + valueB + 1
"#,
    );
    let leaf_fresh = {
        let result = compilation.check(true).expect("leaf edit check should run");
        assert!(result.success(), "leaf edit check should succeed");
        assert!(module_set(result.reused_modules()).contains("a"));
        assert!(module_set(result.reused_modules()).contains("b"));
        assert!(module_set(result.reused_modules()).contains("c"));
        module_set(result.freshly_checked_modules())
    };
    assert_eq!(leaf_fresh, expected_modules(["main"]));

    compilation.set_overlay(
        "src/c.sv",
        r#"valueC : Int
valueC = 4
"#,
    );
    let root_fresh = {
        let result = compilation.check(true).expect("root edit check should run");
        assert!(result.success(), "root edit check should succeed");
        assert!(module_set(result.reused_modules()).contains("b"));
        module_set(result.freshly_checked_modules())
    };
    assert_eq!(root_fresh, expected_modules(["a", "c", "main"]));

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn broken_module_still_reports_independent_module_diagnostics() {
    let project_root = temp_project_root("broken_modules");
    write_source(
        &project_root.join("src/main.sv"),
        r#"import a(valueA)
import b(valueB)

main = 0
"#,
    );
    write_source(
        &project_root.join("src/a.sv"),
        r#"valueA : Int
valueA = True
"#,
    );
    write_source(
        &project_root.join("src/b.sv"),
        r#"valueB : Int
valueB = True
"#,
    );

    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("broken project should check");
    assert!(!result.success(), "broken project should fail");
    assert_eq!(
        module_set(result.freshly_checked_modules()),
        expected_modules(["a", "b"])
    );

    let paths = primary_error_paths(result);
    assert_path_set_contains_file(&paths, "a.sv");
    assert_path_set_contains_file(&paths, "b.sv");

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn source_file_entry_walks_up_to_project_marker() {
    let project_root = temp_project_root("marker");
    write_source(
        &project_root.join("src/main.sv"),
        r#"import a(answer)

main : Int
main = answer
"#,
    );
    write_source(
        &project_root.join("src/a.sv"),
        r#"answer : Int
answer = 41
"#,
    );

    let mut compilation = Compilation::for_source_file(project_root.join("src/a.sv"));
    let result = compilation
        .check(true)
        .expect("project marker check should run");

    assert!(result.success(), "project marker check should succeed");
    let fresh = module_set(result.freshly_checked_modules());
    assert!(fresh.contains("main"));
    assert!(fresh.contains("a"));

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn bare_source_file_without_project_marker_checks_as_one_module() {
    let project_root = temp_project_root("single_file");
    let source_file = project_root.join("scratch.sv");
    write_source(
        &source_file,
        r#"answer : Int
answer = 41
"#,
    );

    let mut compilation = Compilation::for_source_file(&source_file);
    let result = compilation
        .check(true)
        .expect("bare source file should check");

    assert!(result.success(), "bare source file should succeed");
    assert_eq!(
        module_set(result.freshly_checked_modules()),
        expected_modules(["scratch"])
    );

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn measured_2000_line_project_recheck_keeps_dirty_set_small() {
    let project_root = temp_project_root("measurement");
    let mut large_module = String::new();
    for index in 0..1000 {
        large_module.push_str(&format!("value{index} : Int\nvalue{index} = {index}\n"));
    }
    write_source(&project_root.join("src/big.sv"), &large_module);

    let main_template = |literal: usize| {
        format!(
            r#"import big(value999)

main : Int
main = value999 + {literal}
"#
        )
    };
    write_source(&project_root.join("src/main.sv"), &main_template(0));

    let mut compilation = Compilation::new(&project_root);
    assert!(
        compilation
            .check(true)
            .expect("initial measurement check should run")
            .success(),
        "initial measurement check should succeed"
    );

    let mut durations = Vec::new();
    for literal in 1..=20 {
        compilation.set_overlay("src/main.sv", main_template(literal));
        let start = Instant::now();
        let result = compilation
            .check(true)
            .expect("measurement recheck should run");
        durations.push(start.elapsed());
        assert!(result.success(), "measurement recheck should succeed");
        assert_eq!(
            module_set(result.freshly_checked_modules()),
            expected_modules(["main"])
        );
        assert!(module_set(result.reused_modules()).contains("big"));
    }

    let p95 = percentile_95(&mut durations);
    eprintln!(
        "H11 2000-line project main-edit p95: {} ms",
        p95.as_millis()
    );

    let _ = fs::remove_dir_all(project_root);
}

fn percentile_95(durations: &mut [Duration]) -> Duration {
    durations.sort();
    let index = (durations.len() * 95 / 100).min(durations.len().saturating_sub(1));
    durations[index]
}
