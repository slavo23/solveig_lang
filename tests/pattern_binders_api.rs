use std::fs;
use std::path::Path;

use solveig_base::Session;
use solveig_driver::Compilation;
use solveig_syntax::ast::{SurfaceDefinition, SurfaceDoStmt, SurfaceExpr, SurfacePattern};
use solveig_syntax::lowering::parse_source;

use crate::support;

fn temp_root(name: &str) -> support::TestRoot {
    support::temp_root("solveig_pattern_binders", name)
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
        .expect("pattern-binder diagnostic fixture should produce diagnostics");

    !result.success()
        && result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == expected_code)
}

#[test]
fn lambda_pattern_symbols_match_their_body_references() {
    let session = Session::new();
    let source = "main = fun actionError => actionError\n";
    let file = session.add_file("main.sv", source);
    let parsed = parse_source(&session, file, source);

    let SurfaceDefinition::FunDef { body, .. } = &parsed.defs[0].0 else {
        panic!("expected function definition");
    };
    let SurfaceExpr::Abs(params, lambda_body) = &body.0 else {
        panic!("expected lambda, got {body:#?}");
    };
    let (SurfacePattern::Var(param), _, _) = params[0].0 else {
        panic!("expected variable lambda pattern, got {:#?}", params[0].0);
    };
    let SurfaceExpr::Var(reference) = &lambda_body.0 else {
        panic!("expected lambda body reference, got {lambda_body:#?}");
    };

    assert_eq!(reference.0.segments.as_slice(), [param]);
}

#[test]
fn do_pattern_symbols_match_later_statement_references() {
    let session = Session::new();
    let source = "main mmx = do\n    mx <- mmx\n    mx\n";
    let file = session.add_file("main.sv", source);
    let parsed = parse_source(&session, file, source);

    let SurfaceDefinition::FunDef { body, .. } = &parsed.defs[0].0 else {
        panic!("expected function definition");
    };
    let SurfaceExpr::Do(statements) = &body.0 else {
        panic!("expected do expression, got {body:#?}");
    };
    let SurfaceDoStmt::Bind { pattern, .. } = &statements[0].0 else {
        panic!("expected do bind, got {:#?}", statements[0]);
    };
    let SurfacePattern::Var(binder) = pattern.0 else {
        panic!("expected variable do pattern, got {pattern:#?}");
    };
    let SurfaceDoStmt::Expr(last) = &statements[1].0 else {
        panic!("expected final expression, got {:#?}", statements[1]);
    };
    let SurfaceExpr::Var(reference) = &last.0 else {
        panic!("expected final reference, got {last:#?}");
    };

    assert_eq!(reference.0.segments.as_slice(), [binder]);
}

#[test]
fn exhaustive_patterns_work_in_every_local_binder_position() {
    let project_root = temp_root("all_positions");
    let source = r#"data User = User Int Int
data Box a = Box a

lambdaIgnore = (fun _ => 3) 999
lambdaConstructor = (fun (User _ email) => email) (User 1 2)
lambdaTuple = (fun ((a, (b, c)) : (Int, (Int, Int))) => a + b + c) (1, (2, 3))
lambdaNestedConstructor = (fun (Box (User name _)) => name) (Box (User 4 5))

letPatterns =
    let (a, Box (b, c)) = (5, Box (6, 7)) in
    let User _ email = User 8 9 in
    a + b + c + email

doPatterns : Maybe Int
doPatterns = do
    (a, Box b) <- Just (10, Box 11)
    let (c, User _ d) = (12, User 13 14)
    pure (a + b + c + d)

main =
    case doPatterns of
        Nothing => 0
        Just doTotal =>
            lambdaIgnore
                + lambdaConstructor
                + lambdaTuple
                + lambdaNestedConstructor
                + letPatterns
                + doTotal
"#;
    write_source(&project_root.join("src/main.sv"), source);

    let mut compilation = Compilation::new(&project_root);
    let checked = compilation
        .check(true)
        .expect("exhaustive local pattern binder fixture should check");
    assert!(
        checked.success(),
        "diagnostics: {:#?}",
        checked.diagnostics()
    );
    let output = compilation
        .run_project_capture("main")
        .expect("exhaustive local pattern binders should run");

    assert_eq!(output, "89");
}

#[test]
fn old_identifier_and_wildcard_binders_remain_compatible() {
    let project_root = temp_root("old_forms");
    let source = r#"oldLambda = (fun x _ => x) 20 99

oldLet = let x = 21 in x

oldDo : Maybe Int
oldDo = do
    x <- Just 1
    _ <- Just 2
    let y = 0
    pure (x + y)

main =
    case oldDo of
        Nothing => 0
        Just value => oldLambda + oldLet + value
"#;
    write_source(&project_root.join("src/main.sv"), source);

    let mut compilation = Compilation::new(&project_root);
    let checked = compilation
        .check(true)
        .expect("legacy local binder fixture should check");
    assert!(
        checked.success(),
        "diagnostics: {:#?}",
        checked.diagnostics()
    );
    let output = compilation
        .run_project_capture("main")
        .expect("legacy local binder forms should run unchanged");

    assert_eq!(output, "42");
}

#[test]
fn refutable_local_patterns_are_rejected_in_every_position() {
    for (name, source) in [
        (
            "lambda_constructor",
            r#"bad = fun (Just value) => value
main = 0
"#,
        ),
        (
            "let_literal",
            r#"bad = let 1 = 1 in 0
main = bad
"#,
        ),
        (
            "do_bind_constructor",
            r#"bad : Maybe Int
bad = do
    Just value <- Just (Just 1)
    pure value
main = 0
"#,
        ),
        (
            "do_let_list",
            r#"bad : Maybe Int
bad = do
    let [value] = [1]
    pure value
main = 0
"#,
        ),
    ] {
        assert!(
            diagnostics_include_code(source, "TC0016"),
            "{name} should be rejected as non-exhaustive"
        );
    }
}

#[test]
fn repeated_variables_are_rejected_in_nested_local_patterns() {
    for (name, source) in [
        (
            "lambda",
            r#"bad = fun (x, Box (x, _)) => x
main = 0
"#,
        ),
        (
            "let",
            r#"bad = let (x, (x, _)) = (1, (2, 3)) in x
main = bad
"#,
        ),
        (
            "do_bind",
            r#"bad : Maybe Int
bad = do
    (x, Box x) <- Just (1, Box 2)
    pure x
main = 0
"#,
        ),
        (
            "do_let",
            r#"bad : Maybe Int
bad = do
    let (x, (x, _)) = (1, (2, 3))
    pure x
main = 0
"#,
        ),
    ] {
        assert!(
            diagnostics_include_code(source, "RES0017"),
            "{name} should reject a repeated pattern variable"
        );
    }
}
