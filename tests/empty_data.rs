use std::fs;

use solveig_driver::Compilation;

use crate::support;

#[test]
fn empty_case_over_constructor_less_data_checks() {
    let project_root = support::temp_root("solveig_empty_data", "empty_case");
    let source_dir = project_root.join("src");
    fs::create_dir_all(&source_dir).expect("source directory should be created");
    fs::write(
        source_dir.join("main.sv"),
        r#"data Never

absurd : Never -> Int
absurd x = case (x : Never) of

main : Int
main = 0
"#,
    )
    .expect("source fixture should be written");

    let mut compilation = Compilation::new(&project_root);
    let result = compilation.check(true).expect("project should check");

    assert!(result.success(), "{:#?}", result.diagnostics());
}
