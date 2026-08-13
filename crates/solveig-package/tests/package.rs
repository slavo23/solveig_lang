use camino::Utf8Path;
use solveig_package::{Manifest, PackageError, PackageGraph, PackageKind, find_nearest_manifest};
use std::fs;
use tempfile::TempDir;

const BASIC_MANIFEST: &str = r#"[package]
name = "app"
version = "0.1.0"

[language]
entry = "app.main"
"#;

fn utf8_path(directory: &TempDir) -> &Utf8Path {
    Utf8Path::from_path(directory.path()).expect("temporary test paths should be UTF-8")
}

fn write_manifest(root: &Utf8Path, source: &str) {
    fs::write(root.join("solveig.toml"), source).expect("test manifest should be writable");
}

#[test]
fn manifest_round_trip_preserves_all_fields() {
    let source = r#"[package]
name = "app"
version = "1.2.3"

[dependencies]
toolkit = { path = "../toolkit" }

[language]
entry = "app.cli"
"#;
    let manifest = Manifest::parse(source).expect("manifest should parse");
    let rendered = manifest.to_toml().expect("manifest should serialize");
    let reparsed = Manifest::parse(&rendered).expect("serialized manifest should parse");

    assert_eq!(reparsed, manifest);
}

#[test]
fn missing_manifest_is_reported_with_its_path() {
    let directory = TempDir::new().expect("temporary directory should be creatable");
    let error = Manifest::read(utf8_path(&directory)).expect_err("manifest should be missing");

    assert!(matches!(error, PackageError::MissingManifest { .. }));
    assert!(error.to_string().contains("solveig.toml"));
}

#[test]
fn unknown_manifest_keys_are_rejected() {
    let source = r#"[package]
name = "app"
version = "0.1.0"
license = "MIT"

[language]
entry = "app.main"
"#;
    let error = Manifest::parse(source).expect_err("unknown key should be rejected");

    assert!(error.to_string().contains("unknown field `license`"));
}

#[test]
fn package_name_must_be_a_module_segment() {
    let source = r#"[package]
name = "not-valid"
version = "0.1.0"

[language]
entry = "main"
"#;

    let error = Manifest::parse(source).expect_err("invalid package name should fail");

    assert!(error.to_string().contains("package.name"));
    assert!(error.to_string().contains("module segment"));
}

#[test]
fn cyclic_path_dependency_is_reported() {
    let directory = TempDir::new().expect("temporary directory should be creatable");
    let root = utf8_path(&directory);
    let app = root.join("app");
    let library = root.join("library");
    fs::create_dir_all(app.join("src")).expect("app source directory should be creatable");
    fs::create_dir_all(library.join("src")).expect("library source directory should be creatable");
    write_manifest(
        &app,
        r#"[package]
name = "app"
version = "0.1.0"

[dependencies]
library = { path = "../library" }

[language]
entry = "app.main"
"#,
    );
    write_manifest(
        &library,
        r#"[package]
name = "library"
version = "0.1.0"

[dependencies]
app = { path = "../app" }

[language]
entry = "library.main"
"#,
    );

    let error = PackageGraph::load(&app).expect_err("dependency cycle should be rejected");
    assert!(matches!(error, PackageError::DependencyCycle { .. }));
}

#[test]
fn graph_records_layout_and_dependency_edges() {
    let directory = TempDir::new().expect("temporary directory should be creatable");
    let root = utf8_path(&directory);
    let app = root.join("app");
    let library = root.join("library");
    fs::create_dir_all(app.join("src/app")).expect("app source directory should be creatable");
    fs::create_dir_all(library.join("src/library"))
        .expect("library source directory should be creatable");
    fs::write(app.join("src/app/main.sv"), "main = 0").expect("app source should be writable");
    fs::write(library.join("src/library/core.sv"), "answer = 42")
        .expect("library source should be writable");
    write_manifest(
        &app,
        r#"[package]
name = "app"
version = "0.1.0"

[dependencies]
library = { path = "../library" }

[language]
entry = "app.main"
"#,
    );
    write_manifest(
        &library,
        r#"[package]
name = "library"
version = "0.2.0"

[language]
entry = "library.core"
"#,
    );

    let graph = PackageGraph::load(&app).expect("package graph should load");
    let local = graph
        .package(graph.root)
        .expect("local package should exist");
    let dependency_id = local.deps["library"];
    let dependency = graph
        .package(dependency_id)
        .expect("dependency package should exist");

    assert_eq!(local.kind, PackageKind::Local);
    assert_eq!(local.modules.len(), 1);
    assert_eq!(dependency.kind, PackageKind::Dependency);
    assert_eq!(dependency.name, "library");
}

#[test]
fn duplicate_package_names_are_rejected() {
    let directory = TempDir::new().expect("temporary directory should be creatable");
    let root = utf8_path(&directory);
    let app = root.join("app");
    let first = root.join("first");
    let second = root.join("second");
    for package in [&app, &first, &second] {
        fs::create_dir_all(package.join("src"))
            .expect("package source directory should be creatable");
    }
    write_manifest(
        &app,
        r#"[package]
name = "app"
version = "0.1.0"

[dependencies]
first = { path = "../first" }
second = { path = "../second" }

[language]
entry = "main"
"#,
    );
    for package in [&first, &second] {
        write_manifest(
            package,
            r#"[package]
name = "shared"
version = "0.1.0"

[language]
entry = "core"
"#,
        );
    }

    let error = PackageGraph::load(&app).expect_err("package-name collision should fail");

    assert!(matches!(error, PackageError::PackageNameCollision { .. }));
    assert!(error.to_string().contains("shared"));
}

#[test]
fn nearest_manifest_wins() {
    let directory = TempDir::new().expect("temporary directory should be creatable");
    let root = utf8_path(&directory);
    let nested = root.join("workspace/member");
    let source = nested.join("src/member/main.sv");
    fs::create_dir_all(source.parent().expect("source should have a parent"))
        .expect("source directory should be creatable");
    write_manifest(root, BASIC_MANIFEST);
    write_manifest(&nested, BASIC_MANIFEST);
    fs::write(&source, "main = 0").expect("source should be writable");

    assert_eq!(find_nearest_manifest(&source), Some(nested));
}
