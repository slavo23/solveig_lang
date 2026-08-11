use std::fs;
use std::path::Path;

use solveig_driver::Compilation;

mod support;

fn temp_root(name: &str) -> support::TestRoot {
    support::temp_root("solveig_equation_groups", name)
}

fn write_source(path: &Path, source: &str) {
    fs::create_dir_all(
        path.parent()
            .expect("test source should have a parent directory"),
    )
    .expect("test source directory should be creatable");
    fs::write(path, source).expect("test source should be writable");
}

fn diagnostics_include_code(source: &str, expected_code: &str) -> bool {
    let project_root = temp_root(expected_code);
    write_source(&project_root.join("src/main.sv"), source);

    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("diagnostic fixture should check to structured diagnostics");

    !result.success()
        && result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == expected_code)
}

#[test]
fn top_level_equations_preserve_first_match_fallthrough() {
    let project_root = temp_root("fallthrough");
    let file = project_root.join("src/main.sv");
    let source = r#"data MaybeInt = NoInt | SomeInt Int

pick : MaybeInt -> Int -> Int
pick (SomeInt 0) 1 = 10
pick _ 2 = 20
pick (SomeInt n) _ = n
pick NoInt _ = 0

main = pick (SomeInt 0) 2 + pick (SomeInt 7) 9 + pick NoInt 1
"#;
    write_source(&file, source);

    let mut compilation = Compilation::new(&project_root);
    let output = compilation
        .run_project_capture("main")
        .expect("equation group fixture should run");

    assert_eq!(output, "27");
}

#[test]
fn equation_groups_work_with_typeclass_constraints() {
    let project_root = temp_root("typeclasses");
    let file = project_root.join("src/main.sv");
    let source = r#"showMaybe : forall a. Show a => Maybe a -> String
showMaybe Nothing = "none"
showMaybe (Just x) = show x

main = showMaybe (Just 3)
"#;
    write_source(&file, source);

    let mut compilation = Compilation::new(&project_root);
    let output = compilation
        .run_project_capture("main")
        .expect("typeclass equation group fixture should run");

    assert_eq!(output, "\"3\"");
}

#[test]
fn equation_groups_bind_nested_list_patterns_correctly() {
    let project_root = temp_root("list_patterns");
    let file = project_root.join("src/main.sv");
    let source = r#"sumList : [Int] -> Int
sumList [] = 0
sumList (x :: xs) = x + sumList xs

main = sumList [1, 2, 3]
"#;
    write_source(&file, source);

    let mut compilation = Compilation::new(&project_root);
    let output = compilation
        .run_project_capture("main")
        .expect("list-pattern equation group fixture should run");

    assert_eq!(output, "6");
}

#[test]
fn equation_group_diagnostics_cover_group_shape_and_patterns() {
    assert!(diagnostics_include_code(
        r#"bad 0 = 0
bad 1 x = x
main = 0
"#,
        "RES0015",
    ));

    assert!(diagnostics_include_code(
        r#"bad 0 = 0
other = 1
bad 1 = 1
main = other
"#,
        "RES0016",
    ));

    assert!(diagnostics_include_code(
        r#"same x x = x
main = same 1 1
"#,
        "RES0017",
    ));
}

#[test]
fn equation_group_exhaustiveness_uses_all_argument_patterns() {
    assert!(diagnostics_include_code(
        r#"data Bit = O | I

bad : Bit -> Bit -> Int
bad O O = 0

main = bad O O
"#,
        "TC0016",
    ));
}
