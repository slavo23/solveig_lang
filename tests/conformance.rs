//! Fast compiler conformance and whole-system smoke tests.
//! Cases reuse one compilation to retain immutable Prelude/std artifacts while each source is isolated.

#![expect(
    clippy::panic,
    reason = "conformance setup failures need the exact unreadable fixture path"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use solveig_driver::Compilation;
use tempfile::TempDir;

const RUN_CASES: &[&str] = &[
    "smoke_arith",
    "smoke_float",
    "smoke_hrt",
    "smoke_string_char",
    "builtin_primitives",
    "default_methods",
    "do_generic",
    "domain_values",
    "finite_data",
    "fixity_ops",
    "hash_collections",
    "http_surface",
    "instance_method_equations",
    "io_computed",
    "io_map",
    "io_print_str_seq",
    "io_pure",
    "no_show_fallback",
    "operator_classes",
    "operator_sections_backticks",
    "prelude_algebra",
    "tuples",
    "tyvar_shadow_class",
    "prelude_contract",
    "numeric_foundation",
    "bytes_core",
    "collections",
    "json",
    "random_splitmix",
    "testing_generators",
];

const DIAGNOSTIC_CASES: &[(&str, &str)] = &[
    ("parse_error", "PAR0001"),
    ("unbound_variable", "RES0003"),
    ("kind_error_signature", "KIND0004"),
    ("type_mismatch", "TC0005"),
    ("ambiguous_constraint", "TC0021"),
    ("bytes_scanner_hidden", "RES0014"),
    ("class_arity", "RES0005"),
    ("do_final_bind", "PAR0001"),
    ("do_no_monad", "TC0013"),
    ("duplicate_definition", "RES0004"),
    ("fixity_conflict", "RES0008"),
    ("float_pattern", "TC0020"),
    ("http_scope_escape", "TC0005"),
    ("ill_formed_annotation", "TC0011"),
    ("instance_method_mismatch", "TC0015"),
    ("io_error_rename", "RES0003"),
    ("json_limits_constructor_hidden", "RES0003"),
    ("json_number_constructor_hidden", "RES0003"),
    ("missing_import", "RES0002"),
    ("non_exhaustive_instance_equations", "TC0015"),
    ("non_exhaustive_match", "TC0016"),
    ("non_function_application", "TC0009"),
    ("orphan_instance", "RES0021"),
    ("overlapping_instances", "RES0022"),
    ("prelude_socket_hidden", "RES0014"),
    ("typed_hole", "HOLE"),
    ("unknown_operator", "RES0003"),
    ("type_alias_cycle", "RES0013"),
    ("process_scope_escape", "TC0005"),
    ("rank2_missing_constraint", "TC0013"),
    ("std_prim_private", "RES0014"),
    ("stream_scope_escape", "TC0005"),
    ("string_plus_no_num", "TC0013"),
    ("tcp_scope_escape", "TC0005"),
    ("tls_scope_escape", "TC0005"),
    ("two_parse_errors", "PAR0001"),
    ("type_alias_arity", "RES0011"),
    ("type_alias_partial", "RES0012"),
    ("udp_scope_escape", "TC0005"),
    ("unknown_constructor_expr", "RES0003"),
    ("unknown_constructor_pattern", "RES0003"),
    ("unsatisfied_constraint", "TC0013"),
];

#[test]
fn run_corpus_preserves_results_with_one_explicit_compilation_context() {
    let repository = repository_root();
    let project = CorpusProject::new();
    let mut compilation = Compilation::new(project.root());
    let mut failures = Vec::new();

    for case in RUN_CASES {
        let case_root = repository.join("tests/golden/run").join(case);
        let source = read(&case_root.join("src/main.sv"));
        let expected = read(&case_root.join("expected.txt"));
        compilation.set_overlay("src/main.sv", source);

        match compilation.run_project_capture("main") {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!(
                "{case}: output changed\nexpected: {expected:?}\nactual:   {actual:?}"
            )),
            Err(error) => failures.push(format!("{case}: {error}")),
        }
    }

    assert!(
        failures.is_empty(),
        "run conformance failures:\n{}",
        failures.join("\n\n")
    );
}

#[test]
fn diagnostic_corpus_reports_the_expected_compiler_stage() {
    let repository = repository_root();
    let project = CorpusProject::new();
    let mut compilation = Compilation::new(project.root());
    let mut failures = Vec::new();

    for (case, expected_code) in DIAGNOSTIC_CASES {
        let case_root = repository.join("tests/golden/diag").join(case);
        compilation.set_overlay("src/main.sv", read(&case_root.join("src/main.sv")));

        match compilation.check(true) {
            Ok(result)
                if !result.success()
                    && result
                        .diagnostics()
                        .iter()
                        .any(|diagnostic| diagnostic.code == *expected_code) => {}
            Ok(result) => failures.push(format!(
                "{case}: expected {expected_code}, got {:?}",
                result
                    .diagnostics()
                    .iter()
                    .map(|diagnostic| diagnostic.code)
                    .collect::<Vec<_>>()
            )),
            Err(error) => failures.push(format!("{case}: infrastructure failure: {error}")),
        }
    }

    assert!(
        failures.is_empty(),
        "diagnostic conformance failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn reused_and_cold_compilations_have_identical_results() {
    let repository = repository_root();
    let case_root = repository.join("tests/golden/run/smoke_hrt");
    let source = read(&case_root.join("src/main.sv"));

    let reused_project = CorpusProject::new();
    let mut reused = Compilation::new(reused_project.root());
    reused.set_overlay("src/main.sv", "main = 0\n");
    reused
        .run_project_capture("main")
        .expect("warm-up source should run");
    reused.set_overlay("src/main.sv", source.clone());
    let reused_output = reused
        .run_project_capture("main")
        .expect("reused compilation should run the rank-2 case");

    let cold_project = CorpusProject::new();
    let mut cold = Compilation::new(cold_project.root());
    cold.set_overlay("src/main.sv", source);
    let cold_output = cold
        .run_project_capture("main")
        .expect("cold compilation should run the rank-2 case");

    assert_eq!(reused_output, cold_output);
}

#[test]
fn cli_smoke_compiles_verifies_and_runs_a_test_owned_project() {
    let project = repository_root().join("tests/golden/run/smoke_arith");
    let output = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .args(["--verify-core", "--project-root"])
        .arg(project)
        .output()
        .expect("Solveig CLI smoke process should start");

    assert!(
        output.status.success(),
        "CLI smoke failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"42\n");
    assert!(output.stderr.is_empty());
}

struct CorpusProject {
    directory: TempDir,
}

impl CorpusProject {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("corpus directory should be created");
        fs::create_dir(directory.path().join("src"))
            .expect("corpus source directory should be created");
        fs::write(directory.path().join("src/main.sv"), "main = ()\n")
            .expect("corpus entry source should be written");
        Self { directory }
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}
