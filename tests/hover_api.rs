use std::fs;
use std::path::Path;

use solveig_driver::Compilation;

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
