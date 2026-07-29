use std::fs;
use std::path::{Path, PathBuf};

use solveig_driver::Compilation;

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn offset_in_line(source: &str, line: &str, needle: &str) -> usize {
    let line_start = source
        .find(line)
        .unwrap_or_else(|| panic!("hover fixture should contain line `{line}`"));
    let needle_offset = line
        .find(needle)
        .unwrap_or_else(|| panic!("hover fixture line should contain `{needle}`"));

    line_start + needle_offset
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected `{haystack}` to contain `{needle}`"
    );
}

#[test]
fn hover_returns_checked_expression_types_after_run() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/run/smoke_arith");
    let file = project_root.join("src/main.sv");
    let source = fs::read_to_string(&file).expect("hover fixture source should be readable");
    let line = "if total == 10 then 42 else 0";
    let condition_offset = source
        .find(line)
        .map(|start| start + "if ".len())
        .expect("hover fixture should contain the target expression");
    let then_offset = source
        .find(line)
        .map(|start| start + "if total == 10 then ".len())
        .expect("hover fixture should contain the then literal");
    let else_offset = source
        .find(line)
        .map(|start| start + "if total == 10 then 42 else ".len())
        .expect("hover fixture should contain the else literal");

    let mut compilation = Compilation::new(&project_root);
    let output = compilation
        .run_project_capture("main")
        .expect("hover fixture should run before hover");

    assert_eq!(output, "42");
    assert_eq!(
        compilation.hover(&file, condition_offset),
        Some("Bool".to_string())
    );
    assert_eq!(
        compilation.hover(&file, then_offset),
        Some("Int".to_string())
    );
    assert_eq!(
        compilation.hover(&file, else_offset),
        Some("Int".to_string())
    );
}

#[test]
fn hover_preserves_long_source_type_variable_names() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/types/checker_api");
    let file = project_root.join("src/main.sv");
    let source = fs::read_to_string(&file).expect("hover fixture source should be readable");
    let line = "longElementId value = value";
    let value_offset = source
        .find(line)
        .map(|start| {
            start
                + line
                    .rfind("value")
                    .expect("hover line should contain a RHS value")
        })
        .expect("hover fixture should contain the target expression");

    let mut compilation = Compilation::new(&project_root);
    let output = compilation
        .run_project_capture("main")
        .expect("checker fixture should run before hover");

    assert_eq!(output, "42");
    let schemes = solveig_driver::checked_project_schemes(&project_root)
        .expect("checker fixture should check before hover");

    assert_eq!(
        schemes.get("main.longElementId"),
        Some(&"∀ (element : *). element -> element".to_string())
    );
    assert_eq!(
        compilation.hover(&file, value_offset),
        Some("element".to_string())
    );
}

#[test]
fn hover_info_covers_h13_semantic_targets() {
    let project_root = fixture_root("tests/fixtures/hover");
    let main_file = project_root.join("src/main.sv");
    let lib_file = project_root.join("src/hoverLib.sv");
    let main_source = fs::read_to_string(&main_file).expect("hover fixture source should load");
    let lib_source = fs::read_to_string(&lib_file).expect("hover lib fixture should load");
    let mut compilation = Compilation::new(&project_root);
    let result = compilation.check(true).expect("hover fixture should check");

    assert!(
        result.success(),
        "hover fixture diagnostics: {:?}",
        result.diagnostics()
    );

    let local = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    inner + 1", "inner"),
        )
        .expect("local binder hover should exist");
    assert_eq!(local.signature(), "Int");

    let top_level = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "topLevel : Int -> Int", "topLevel"),
        )
        .expect("top-level hover should exist");
    assert_eq!(top_level.signature(), "topLevel : Int -> Int");

    let imported = compilation
        .hover_info(
            &main_file,
            offset_in_line(
                &main_source,
                "topLevel n = documentedIncrement n",
                "documentedIncrement",
            ),
        )
        .expect("imported function hover should exist");
    assert_eq!(imported.signature(), "documentedIncrement : Int -> Int");
    assert_eq!(imported.documentation(), Some("Adds one to an integer."));
    assert!(
        imported
            .provenance()
            .is_some_and(|provenance| provenance.contains("hoverLib")),
        "imported function hover should include provenance: {imported:#?}"
    );

    let imported_type = compilation
        .hover_info(
            &main_file,
            offset_in_line(
                &main_source,
                "useImported : Documented -> Documented",
                "Documented",
            ),
        )
        .expect("imported type hover should exist");
    assert_eq!(imported_type.signature(), "type Documented : *");
    assert!(
        imported_type
            .provenance()
            .is_some_and(|provenance| provenance.contains("hoverLib")),
        "imported type hover should include provenance: {imported_type:#?}"
    );

    let operator = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    inner + 1", "+"),
        )
        .expect("operator hover should exist");
    assert_contains(operator.signature(), "class Num a");
    assert_contains(operator.signature(), "+ :");

    let constructor = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    case MkLocalBox 41 of", "MkLocalBox"),
        )
        .expect("constructor hover should exist");
    assert_eq!(constructor.signature(), "MkLocalBox : Int -> LocalBox");

    let scrutinee = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    case value of", "value"),
        )
        .expect("case scrutinee hover should exist");
    assert_eq!(scrutinee.signature(), "Documented");

    let method = compilation
        .hover_info(
            &lib_file,
            offset_in_line(&lib_source, "    describe : a -> String", "describe"),
        )
        .expect("class method hover should exist");
    assert_contains(method.signature(), "class Describe a");
    assert_contains(method.signature(), "describe :");
    assert_eq!(
        method.documentation(),
        Some("Converts a value into a friendly label.")
    );

    let do_expression = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    pure (x + y)", "pure"),
        )
        .expect("do-block expression hover should exist");
    assert_contains(do_expression.signature(), "Applicative f");
    assert_contains(do_expression.signature(), "pure :");

    let module = compilation
        .hover_info(
            &main_file,
            offset_in_line(
                &main_source,
                "import hoverLib(documentedIncrement, Documented, MkDocumented, Describe)",
                "hoverLib",
            ),
        )
        .expect("module hover should exist");
    assert_eq!(module.signature(), "module hoverLib");
    assert!(
        module
            .documentation()
            .is_some_and(|path| path.ends_with("tests/fixtures/hover/src/hoverLib.sv")),
        "module hover should include the imported path"
    );
}
