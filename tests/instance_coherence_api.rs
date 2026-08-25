use std::fs;
use std::path::Path;

use solveig_driver::Compilation;

use crate::support;

fn write_source(path: &Path, source: &str) {
    fs::create_dir_all(path.parent().expect("test source must have a parent"))
        .expect("test source directory should be creatable");
    fs::write(path, source).expect("test source should be writable");
}

fn write_manifest(root: &Path, package: &str) {
    write_source(
        &root.join("solveig.toml"),
        &format!(
            "[package]\nname = \"{package}\"\nversion = \"0.1.0\"\n\n[language]\nentry = \"main\"\n"
        ),
    );
}

#[test]
fn package_instances_are_available_without_imports_in_every_module_order() {
    for instance_module in ["a_instances", "z_instances"] {
        let package = format!("coherent_{instance_module}");
        let project = support::temp_root("solveig_instances", instance_module);
        write_manifest(&project, &package);
        write_source(
            &project.join("src/defs.sv"),
            "export (Need)\n\nclass Need a where\n    need : a -> Int\n",
        );
        write_source(
            &project.join(format!("src/{instance_module}.sv")),
            &format!(
                "import {package}.defs(Need)\n\ninstance Need Int where\n    need value = value\n"
            ),
        );
        write_source(
            &project.join("src/main.sv"),
            &format!("import {package}.defs(Need)\n\nmain : Int\nmain = need 42\n"),
        );

        let mut compilation = Compilation::new(&project);
        let result = compilation
            .check(true)
            .expect("package-global instance project should check");

        assert!(
            result.success(),
            "instance module `{instance_module}` was order-sensitive: {:#?}",
            result.diagnostics()
        );
    }
}

#[test]
fn alias_expansion_cannot_disguise_an_orphan_instance() {
    for instance_module in ["a_orphan", "z_orphan"] {
        let project = support::temp_root("solveig_instances", instance_module);
        write_manifest(&project, "alias_orphan");
        write_source(
            &project.join(format!("src/{instance_module}.sv")),
            r#"type LocalInt = Int

instance Semigroup LocalInt where
    combine left right = left + right
"#,
        );
        write_source(&project.join("src/main.sv"), "main = 0\n");

        let mut compilation = Compilation::new(&project);
        let result = compilation
            .check(true)
            .expect("orphan instance should produce a diagnostic");

        assert!(!result.success());
        assert_eq!(
            result
                .diagnostics()
                .iter()
                .filter(|diagnostic| diagnostic.code == "RES0021")
                .count(),
            1,
            "orphan module `{instance_module}` was order-sensitive: {:#?}",
            result.diagnostics()
        );
    }
}

#[test]
fn unused_cross_module_overlaps_are_reported_once() {
    for general_module in ["a_general", "z_general"] {
        let specific_module = if general_module == "a_general" {
            "z_specific"
        } else {
            "a_specific"
        };
        let project = support::temp_root("solveig_instances", general_module);
        write_manifest(&project, "overlap");
        write_source(
            &project.join("src/defs.sv"),
            "export (Need)\n\nclass Need a where\n    need : a -> Int\n",
        );
        write_source(
            &project.join(format!("src/{general_module}.sv")),
            "import overlap.defs(Need)\n\ninstance Need a where\n    need value = 0\n",
        );
        write_source(
            &project.join(format!("src/{specific_module}.sv")),
            "import overlap.defs(Need)\n\ninstance Need Int where\n    need value = value\n",
        );
        write_source(&project.join("src/main.sv"), "main = 0\n");

        let mut compilation = Compilation::new(&project);
        let result = compilation
            .check(true)
            .expect("overlapping instances should produce a diagnostic");

        assert!(!result.success());
        assert_eq!(
            result
                .diagnostics()
                .iter()
                .filter(|diagnostic| diagnostic.code == "RES0022")
                .count(),
            1,
            "general instance module `{general_module}` was order-sensitive: {:#?}",
            result.diagnostics()
        );
    }
}

#[test]
fn editing_an_instance_module_invalidates_unrelated_consumers() {
    let project = support::temp_root("solveig_instances", "incremental_instance_graph");
    write_manifest(&project, "coherence_edit");
    write_source(
        &project.join("src/defs.sv"),
        "export (Need)\n\nclass Need a where\n    need : a -> Int\n",
    );
    write_source(
        &project.join("src/z_instances.sv"),
        "import coherence_edit.defs(Need)\n\ninstance Need Int where\n    need value = value\n",
    );
    write_source(
        &project.join("src/main.sv"),
        "import coherence_edit.defs(Need)\n\nmain : Int\nmain = need 42\n",
    );

    let mut compilation = Compilation::new(&project);
    let initial = compilation.check(true).expect("initial check should run");
    assert!(initial.success(), "{:#?}", initial.diagnostics());

    compilation.set_overlay(
        "src/z_instances.sv",
        "import coherence_edit.defs(Need)\n\ninstance Need Float where\n    need value = 0\n",
    );
    let changed = compilation
        .check(true)
        .expect("instance edit check should run");

    assert!(!changed.success());
    assert!(
        changed
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "TC0013"),
        "{:#?}",
        changed.diagnostics()
    );
    assert!(
        changed
            .freshly_checked_modules()
            .iter()
            .any(|module| module.ends_with(".main")),
        "consumer was incorrectly reused: {:#?}",
        changed.reused_modules()
    );
}
