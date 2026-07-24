use std::fs;
use std::path::Path;

use solveig_driver::Compilation;

#[test]
fn hover_returns_innermost_checked_expression_type_after_run() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/run/smoke_arith");
    let file = project_root.join("src/main.sv");
    let source = fs::read_to_string(&file).expect("hover fixture source should be readable");
    let line = "if total == 10 then 42 else 0";
    let offset = source
        .find(line)
        .map(|start| start + "if ".len())
        .expect("hover fixture should contain the target expression");

    let compilation = Compilation::new(&project_root);
    let output = compilation
        .run_project_capture("main")
        .expect("hover fixture should run before hover");

    assert_eq!(output, "42");
    assert_eq!(compilation.hover(&file, offset), Some("Int".to_string()));
}
