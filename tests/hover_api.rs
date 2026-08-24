#![expect(
    clippy::panic,
    reason = "integration tests use panic messages to explain fixture lookup failures"
)]

use std::fs;
use std::path::{Path, PathBuf};

use solveig_driver::Compilation;
use solveig_ide::CompilationIdeExt;
use solveig_syntax::lexer::lex;

use crate::support;

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn temp_root(name: &str) -> support::TestRoot {
    support::temp_root("solveig_hover", name)
}

fn write_source(path: &Path, source: &str) {
    fs::create_dir_all(
        path.parent()
            .expect("test source should have a parent directory"),
    )
    .expect("test source directory should be creatable");
    fs::write(path, source).expect("test source should be writable");
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

fn offset_in_line_last(source: &str, line: &str, needle: &str) -> usize {
    let line_start = source
        .find(line)
        .unwrap_or_else(|| panic!("hover fixture should contain line `{line}`"));
    let needle_offset = line
        .rfind(needle)
        .unwrap_or_else(|| panic!("hover fixture line should contain `{needle}`"));

    line_start + needle_offset
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected `{haystack}` to contain `{needle}`"
    );
}

fn hover_audit_for_lines(
    compilation: &mut Compilation,
    file: &Path,
    source: &str,
    lines: &[&str],
) -> String {
    let tokens = lex(solveig_base::FileId(0), source).expect("hover audit source should lex");
    let line_ranges = lines
        .iter()
        .map(|line| {
            let start = source
                .find(line)
                .unwrap_or_else(|| panic!("hover audit source should contain line `{line}`"));
            (start, start + line.len())
        })
        .collect::<Vec<_>>();
    let mut out = String::new();

    for token in tokens {
        if token.is_synthetic()
            || token.kind.is_trivia()
            || !line_ranges
                .iter()
                .any(|(start, end)| *start <= token.start() && token.end() <= *end)
        {
            continue;
        }

        let text = token.source_text(source);
        if text.is_empty() {
            continue;
        }

        let signature = compilation.hover_info(file, token.start()).map_or_else(
            || "<none>".to_string(),
            |info| info.signature().replace('\n', "\\n"),
        );

        out.push_str(&format!(
            "{:>3}:{:<3} `{}` => {}\n",
            token.line + 1,
            token.col + 1,
            text,
            signature
        ));
    }

    out
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
        Some("total : Int".to_string())
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
fn hover_renders_prelude_type_alias_at_reference() {
    let project_root = fixture_root("tests/golden/run/io_hello");
    let file = project_root.join("src/main.sv");
    let source = fs::read_to_string(&file).expect("IO fixture source should be readable");
    let mut compilation = Compilation::new(&project_root);
    let result = compilation.check(true).expect("IO fixture should check");

    assert!(
        result.success(),
        "IO fixture diagnostics: {:?}",
        result.diagnostics()
    );

    let hover = compilation
        .hover_info(&file, offset_in_line(&source, "main : Task ()", "Task"))
        .expect("Task alias reference should have hover");

    assert_eq!(hover.signature(), "type Task a = IO IOError a");
    assert_eq!(
        hover.documentation(),
        Some("An IO action using the built-in error vocabulary.")
    );
    assert!(
        hover
            .provenance()
            .is_some_and(|provenance| provenance.contains("prelude.builtins")),
        "Task hover should include Prelude provenance: {hover:#?}"
    );
    assert_eq!(hover.span(), 7..11);
}

#[test]
fn hover_covers_parenthesized_operator_definitions_and_references() {
    let project_root = temp_root("operator_hovers");
    let file = project_root.join("scratch.sv");
    let source = r#"infixr 5 (++)

(++) = append

prepend : forall a. a -> [a] -> [a]
prepend = (::)

main : [Int]
main = [1] ++ [2]
"#;
    write_source(&file, source);

    let mut compilation = Compilation::for_source_file(&file);
    let result = compilation.check(true).expect("check should finish");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let expected = "(++) : forall a. [a] -> [a] -> [a]";
    let definition_open = offset_in_line(source, "(++) = append", "(");
    let definition_open_hover = compilation
        .hover_info(&file, definition_open)
        .expect("operator definition opening paren should hover");
    assert_eq!(definition_open_hover.signature(), expected);
    assert_eq!(
        definition_open_hover.span(),
        definition_open..definition_open + "(++)".len()
    );

    let definition_operator = offset_in_line(source, "(++) = append", "++");
    let definition_operator_hover = compilation
        .hover_info(&file, definition_operator)
        .expect("operator definition token should hover");
    assert_eq!(definition_operator_hover.signature(), expected);
    assert_eq!(
        definition_operator_hover.span(),
        definition_open..definition_open + "(++)".len()
    );

    let reference_operator = offset_in_line(source, "main = [1] ++ [2]", "++");
    let reference_hover = compilation
        .hover_info(&file, reference_operator)
        .expect("operator reference should hover");
    assert_eq!(reference_hover.signature(), expected);
    assert_eq!(
        reference_hover.span(),
        reference_operator..reference_operator + "++".len()
    );

    let cons_operator = offset_in_line(source, "prepend = (::)", "::");
    let cons_hover = compilation
        .hover_info(&file, cons_operator)
        .expect("first-class cons operator should hover");
    assert_contains(cons_hover.signature(), "Cons :");
    assert_contains(cons_hover.signature(), "a -> [a] -> [a]");

    drop(project_root);
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
        Some(&"forall element. element -> element".to_string())
    );
    assert_eq!(
        compilation.hover(&file, value_offset),
        Some("value : element".to_string())
    );
}

#[test]
fn hover_covers_forall_signature_binder_kinds() {
    let project_root = temp_root("forall_binder_kinds");
    let file = project_root.join("src/main.sv");
    let source = r#"data Box a = Box a

higher : forall f a. f a -> f a
higher x = x

main = 0
"#;
    write_source(&file, source);

    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("forall binder hover fixture should check");

    assert!(
        result.success(),
        "forall binder diagnostics: {:?}",
        result.diagnostics()
    );

    let line = "higher : forall f a. f a -> f a";
    let higher = compilation
        .hover_info(&file, offset_in_line(source, line, "higher"))
        .expect("function name should retain exact source signature");
    assert_eq!(higher.signature(), "higher : forall f a. f a -> f a");

    let f_binder = compilation
        .hover_info(&file, offset_in_line(source, line, "f a."))
        .expect("higher-kinded forall binder should hover");
    assert_eq!(f_binder.signature(), "f : * -> *");

    let a_binder = compilation
        .hover_info(&file, offset_in_line(source, line, "a. f"))
        .expect("star-kinded forall binder should hover");
    assert_eq!(a_binder.signature(), "a : *");

    let f_use = compilation
        .hover_info(&file, offset_in_line_last(source, line, "f a"))
        .expect("higher-kinded forall variable use should hover");
    assert_eq!(f_use.signature(), "f : * -> *");

    let a_use_offset = source
        .find(line)
        .and_then(|start| line.rfind("f a").map(|offset| start + offset + "f ".len()))
        .expect("forall fixture should contain a body type variable use");
    let a_use = compilation
        .hover_info(&file, a_use_offset)
        .expect("star-kinded forall variable use should hover");
    assert_eq!(a_use.signature(), "a : *");
}

#[test]
fn hover_on_typed_hole_uses_the_hole_span_and_type() {
    let project_root = temp_root("typed_hole_hover");
    let file = project_root.join("src/main.sv");
    let source = r#"main : Bool
main = if True then ?? else False
"#;
    write_source(&file, source);

    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("typed-hole hover fixture should check to diagnostics");

    assert!(
        !result.success(),
        "typed-hole fixture should not be executable"
    );
    assert!(
        result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "HOLE"),
        "typed-hole diagnostic missing: {:?}",
        result.diagnostics()
    );

    let hole_start = offset_in_line(source, "main = if True then ?? else False", "??");
    let hover = compilation
        .hover_info(&file, hole_start)
        .expect("typed hole should have hover info");

    assert_eq!(hover.signature(), "found hole of type `Bool`");
    let markdown = hover
        .markdown()
        .expect("typed-hole hover should provide diagnostic markdown");
    assert_contains(markdown, "**Unfilled hole**");
    assert_contains(markdown, "found hole of type `Bool`");
    assert_contains(markdown, "Expected type: `Bool`");
    assert_eq!(hover.span(), hole_start..hole_start + 2);

    drop(project_root);
}

#[test]
fn hover_on_underscore_hole_uses_the_hole_span_and_type() {
    let project_root = temp_root("underscore_hole_hover");
    let file = project_root.join("src/main.sv");
    let source = r#"main : Bool
main = if True then _ else False
"#;
    write_source(&file, source);

    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("underscore-hole hover fixture should check to diagnostics");

    assert!(
        result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "HOLE"),
        "underscore-hole diagnostic missing: {:?}",
        result.diagnostics()
    );

    let hole_start = offset_in_line(source, "main = if True then _ else False", "_");
    let hover = compilation
        .hover_info(&file, hole_start)
        .expect("underscore hole should have hover info");

    assert_eq!(hover.signature(), "found hole of type `Bool`");
    let markdown = hover
        .markdown()
        .expect("underscore-hole hover should provide diagnostic markdown");
    assert_contains(markdown, "**Unfilled hole**");
    assert_contains(markdown, "found hole of type `Bool`");
    assert_contains(markdown, "Expected type: `Bool`");
    assert_eq!(hover.span(), hole_start..hole_start + 1);

    drop(project_root);
}

#[test]
fn hover_on_hole_inside_map_constructor_lambda_uses_hole_type() {
    let project_root = fixture_root("examples/smoke_suite");
    let file = project_root.join("src/main.sv");
    let original = fs::read_to_string(&file).expect("smoke suite main source should load");
    let source = original.replace("Entry iVal iVal", "Entry ?? iVal");

    let mut compilation = Compilation::new(&project_root);
    compilation.set_overlay(&file, source.clone());
    let result = compilation
        .check(true)
        .expect("smoke-suite hole fixture should check to diagnostics");

    assert!(
        result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "HOLE"),
        "constructor-lambda hole diagnostic missing: {:?}",
        result.diagnostics()
    );

    let line = "    let intEntries = (map (fun iVal => Entry ?? iVal) (rangeInclusive 1 20000)) in";
    let hole_start = offset_in_line(&source, line, "??");
    let hover = compilation
        .hover_info(&file, hole_start)
        .expect("constructor-lambda hole should have hover info");

    assert_eq!(hover.signature(), "found hole of type `Int`");
    let markdown = hover
        .markdown()
        .expect("constructor-lambda hole should provide diagnostic markdown");
    assert_contains(markdown, "**Unfilled hole**");
    assert_contains(markdown, "found hole of type `Int`");
    assert_contains(markdown, "Expected type: `Int`");
    assert_contains(markdown, "Bindings in scope");
    assert_contains(markdown, "iVal : Int");
    assert_contains(markdown, "Valid fills include");
    assert_contains(markdown, "`iVal`");
    assert_eq!(hover.span(), hole_start..hole_start + 2);
}

#[test]
fn hover_on_second_map_constructor_lambda_hole_shows_fill_suggestions() {
    let project_root = fixture_root("examples/smoke_suite");
    let file = project_root.join("src/main.sv");
    let original = fs::read_to_string(&file).expect("smoke suite main source should load");
    let source = original.replace("Entry iVal iVal", "Entry iVal ??");

    let mut compilation = Compilation::new(&project_root);
    compilation.set_overlay(&file, source.clone());
    let result = compilation
        .check(true)
        .expect("smoke-suite second-arg hole fixture should check to diagnostics");

    assert!(
        result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "HOLE"),
        "constructor-lambda second-arg hole diagnostic missing: {:?}",
        result.diagnostics()
    );

    let line = "    let intEntries = (map (fun iVal => Entry iVal ??) (rangeInclusive 1 20000)) in";
    let hole_start = offset_in_line(&source, line, "??");
    let hover = compilation
        .hover_info(&file, hole_start)
        .expect("constructor-lambda second-arg hole should have hover info");

    assert_eq!(hover.signature(), "found hole of type `Int`");
    let markdown = hover
        .markdown()
        .expect("constructor-lambda second-arg hole should provide diagnostic markdown");
    assert_contains(markdown, "**Unfilled hole**");
    assert_contains(markdown, "found hole of type `Int`");
    assert_contains(markdown, "Expected type: `Int`");
    assert_contains(markdown, "Bindings in scope");
    assert_contains(markdown, "iVal : Int");
    assert_contains(markdown, "Valid fills include");
    assert_contains(markdown, "`iVal`");
    assert_eq!(hover.span(), hole_start..hole_start + 2);
}

#[test]
fn hover_covers_top_level_equation_patterns_and_bodies() {
    let project_root = temp_root("equation_group_hovers");
    let file = project_root.join("src/main.sv");
    let source = r#"data Card = Blank | Face Int

score : Card -> Int -> Int
score (Face n) 0 = n
score _ total = total

main = score (Face 3) 0
"#;
    write_source(&file, source);

    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("equation hover fixture should check");

    assert!(
        result.success(),
        "equation hover diagnostics: {:?}",
        result.diagnostics()
    );

    let constructor_start = offset_in_line(source, "score (Face n) 0 = n", "Face");
    let constructor = compilation
        .hover_info(&file, constructor_start)
        .expect("pattern constructor should hover");
    assert_eq!(constructor.signature(), "Face : Int -> Card");
    assert_eq!(constructor.span(), constructor_start..constructor_start + 4);

    let pattern_binder_start = offset_in_line(source, "score (Face n) 0 = n", "n)");
    let pattern_binder = compilation
        .hover_info(&file, pattern_binder_start)
        .expect("pattern binder should hover");
    assert_eq!(pattern_binder.signature(), "n : Int");
    assert_eq!(
        pattern_binder.span(),
        pattern_binder_start..pattern_binder_start + 1
    );

    let body_binder_start = offset_in_line_last(source, "score (Face n) 0 = n", "n");
    let body_binder = compilation
        .hover_info(&file, body_binder_start)
        .expect("body reference to pattern binder should hover");
    assert_eq!(body_binder.signature(), "n : Int");
    assert_eq!(body_binder.span(), body_binder_start..body_binder_start + 1);

    let wildcard_start = offset_in_line(source, "score _ total = total", "_");
    let wildcard = compilation
        .hover_info(&file, wildcard_start)
        .expect("equation wildcard should hover");
    assert_eq!(wildcard.signature(), "_ : Card");
    assert_eq!(wildcard.span(), wildcard_start..wildcard_start + 1);

    let later_pattern_binder_start = offset_in_line(source, "score _ total = total", "total");
    let later_pattern_binder = compilation
        .hover_info(&file, later_pattern_binder_start)
        .expect("later equation binder should hover");
    assert_eq!(later_pattern_binder.signature(), "total : Int");
    assert_eq!(
        later_pattern_binder.span(),
        later_pattern_binder_start..later_pattern_binder_start + "total".len()
    );

    let later_body_binder_start = offset_in_line_last(source, "score _ total = total", "total");
    let later_body_binder = compilation
        .hover_info(&file, later_body_binder_start)
        .expect("later equation body reference should hover");
    assert_eq!(later_body_binder.signature(), "total : Int");
    assert_eq!(
        later_body_binder.span(),
        later_body_binder_start..later_body_binder_start + "total".len()
    );

    drop(project_root);
}

#[test]
fn hover_covers_instance_method_equation_patterns_and_bodies() {
    let project_root = temp_root("instance_equation_group_hovers");
    let file = project_root.join("src/main.sv");
    let source = r#"data Card = Blank | Face Int

class Score a where
    score : a -> Int -> Int

instance Score Card where
    score (Face n) 0 = n
    score _ total = total

main = score (Face 3) 0
"#;
    write_source(&file, source);

    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("instance equation hover fixture should check");

    assert!(
        result.success(),
        "instance equation hover diagnostics: {:?}",
        result.diagnostics()
    );

    let constructor_start = offset_in_line(source, "    score (Face n) 0 = n", "Face");
    let constructor = compilation
        .hover_info(&file, constructor_start)
        .expect("instance pattern constructor should hover");
    assert_eq!(constructor.signature(), "Face : Int -> Card");

    let pattern_binder_start = offset_in_line(source, "    score (Face n) 0 = n", "n)");
    let pattern_binder = compilation
        .hover_info(&file, pattern_binder_start)
        .expect("instance equation pattern binder should hover");
    assert_eq!(pattern_binder.signature(), "n : Int");

    let body_binder_start = offset_in_line_last(source, "    score (Face n) 0 = n", "n");
    let body_binder = compilation
        .hover_info(&file, body_binder_start)
        .expect("instance equation body binder reference should hover");
    assert_eq!(body_binder.signature(), "n : Int");

    let wildcard_start = offset_in_line(source, "    score _ total = total", "_");
    let wildcard = compilation
        .hover_info(&file, wildcard_start)
        .expect("instance equation wildcard should hover");
    assert_eq!(wildcard.signature(), "_ : Card");

    let later_pattern_binder_start = offset_in_line(source, "    score _ total = total", "total");
    let later_pattern_binder = compilation
        .hover_info(&file, later_pattern_binder_start)
        .expect("later instance equation binder should hover");
    assert_eq!(later_pattern_binder.signature(), "total : Int");

    let later_body_binder_start = offset_in_line_last(source, "    score _ total = total", "total");
    let later_body_binder = compilation
        .hover_info(&file, later_body_binder_start)
        .expect("later instance equation body reference should hover");
    assert_eq!(later_body_binder.signature(), "total : Int");

    drop(project_root);
}

#[test]
fn hover_covers_nested_tuple_delimiters_and_elements() {
    let project_root = temp_root("nested_tuple_hovers");
    let file = project_root.join("src/main.sv");
    let source = r#"value : (Int, (String, Bool))
value = (1, ("nested", True))

inspect : (Int, (String, Bool)) -> Int
inspect (count, (name, active)) = count

class Marker a where
    marker : a -> Int

instance Marker (Int, (String, Bool)) where
    marker (count, (name, active)) = count

fromCase : (Int, (String, Bool)) -> String
fromCase value =
    case value of
        (count, (name, active)) => name

main = (inspect value, marker value, fromCase value)
"#;
    write_source(&file, source);

    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("nested tuple hover fixture should check");
    assert!(
        result.success(),
        "nested tuple hover diagnostics: {:?}",
        result.diagnostics()
    );

    let value_line = "value = (1, (\"nested\", True))";
    for (needle, expected) in [
        ("(1", "(Int, (String, Bool))"),
        (")", "(Int, (String, Bool))"),
        ("(\"nested", "(String, Bool)"),
        ("\"nested\"", "String"),
        ("True", "True : Bool"),
    ] {
        let offset = if needle == ")" {
            offset_in_line_last(source, value_line, needle)
        } else {
            offset_in_line(source, value_line, needle)
        };
        let hover = compilation
            .hover_info(&file, offset)
            .unwrap_or_else(|| panic!("expression tuple token `{needle}` should hover"));
        assert_eq!(hover.signature(), expected, "expression token `{needle}`");
    }
    let inner_value_close = offset_in_line(source, value_line, "True)") + "True".len();
    assert_eq!(
        compilation
            .hover_info(&file, inner_value_close)
            .expect("inner expression closing parenthesis should hover")
            .signature(),
        "(String, Bool)"
    );

    for pattern_line in [
        "inspect (count, (name, active)) = count",
        "    marker (count, (name, active)) = count",
        "        (count, (name, active)) => name",
    ] {
        let outer_open = offset_in_line(source, pattern_line, "(count");
        let outer_close = offset_in_line_last(source, pattern_line, ")");
        let inner_open = offset_in_line(source, pattern_line, "(name");
        let inner_close = offset_in_line(source, pattern_line, "active)") + "active".len();

        for (offset, expected) in [
            (outer_open, "(Int, (String, Bool))"),
            (outer_close, "(Int, (String, Bool))"),
            (inner_open, "(String, Bool)"),
            (inner_close, "(String, Bool)"),
        ] {
            assert_eq!(
                compilation
                    .hover_info(&file, offset)
                    .unwrap_or_else(|| panic!("tuple delimiter in `{pattern_line}` should hover"))
                    .signature(),
                expected
            );
        }

        for (needle, expected) in [
            ("count", "count : Int"),
            ("name", "name : String"),
            ("active", "active : Bool"),
        ] {
            assert_eq!(
                compilation
                    .hover_info(&file, offset_in_line(source, pattern_line, needle))
                    .unwrap_or_else(|| panic!("tuple element `{needle}` should hover"))
                    .signature(),
                expected
            );
        }
    }

    drop(project_root);
}

#[test]
fn hover_covers_patterns_in_lambdas_lets_and_do_blocks() {
    let project_root = temp_root("local_pattern_binder_hovers");
    let file = project_root.join("src/main.sv");
    let source = r#"data User = User String Int
data Box a = Box a

fromLambda : Box (String, Int) -> Int
fromLambda = fun (Box (name, count)) => count

fromLet : Int
fromLet =
    let (title, User name age) = ("admin", User "Ada" 42) in
    age

fromDo : Maybe Int
fromDo = do
    (label, Box amount) <- Just ("total", Box 7)
    let User userName userAge = User label amount
    pure (amount + userAge)

main = fromLambda (Box ("items", 1)) + fromLet
"#;
    write_source(&file, source);

    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("local pattern binder hover fixture should check");
    assert!(
        result.success(),
        "local pattern binder hover diagnostics: {:?}",
        result.diagnostics()
    );

    for (line, needle, expected) in [
        (
            "fromLambda = fun (Box (name, count)) => count",
            "name",
            "name : String",
        ),
        (
            "fromLambda = fun (Box (name, count)) => count",
            "count",
            "count : Int",
        ),
        (
            "    let (title, User name age) = (\"admin\", User \"Ada\" 42) in",
            "title",
            "title : String",
        ),
        (
            "    let (title, User name age) = (\"admin\", User \"Ada\" 42) in",
            "age",
            "age : Int",
        ),
        (
            "    (label, Box amount) <- Just (\"total\", Box 7)",
            "amount",
            "amount : Int",
        ),
        (
            "    let User userName userAge = User label amount",
            "userAge",
            "userAge : Int",
        ),
    ] {
        let hover = compilation
            .hover_info(&file, offset_in_line(source, line, needle))
            .unwrap_or_else(|| panic!("local pattern binder `{needle}` should hover"));
        assert_eq!(hover.signature(), expected, "binder `{needle}`");
    }

    for (line, needle, expected) in [
        (
            "fromLambda = fun (Box (name, count)) => count",
            "=> count",
            "count : Int",
        ),
        ("    age", "age", "age : Int"),
        ("    pure (amount + userAge)", "amount", "amount : Int"),
        ("    pure (amount + userAge)", "userAge", "userAge : Int"),
    ] {
        let token = needle.strip_prefix("=> ").unwrap_or(needle);
        let offset = if needle.starts_with("=> ") {
            offset_in_line(source, line, needle) + 3
        } else {
            offset_in_line(source, line, needle)
        };
        let hover = compilation
            .hover_info(&file, offset)
            .unwrap_or_else(|| panic!("local pattern reference `{token}` should hover"));
        assert_eq!(hover.signature(), expected, "reference `{token}`");
    }

    let lambda_line = "fromLambda = fun (Box (name, count)) => count";
    for offset in [
        offset_in_line(source, lambda_line, "(name"),
        offset_in_line(source, lambda_line, "count)") + "count".len(),
    ] {
        assert_eq!(
            compilation
                .hover_info(&file, offset)
                .expect("nested lambda tuple delimiter should hover")
                .signature(),
            "(String, Int)"
        );
    }

    drop(project_root);
}

#[test]
fn hole_diagnostics_and_hovers_survive_incremental_recheck() {
    let project_root = fixture_root("examples/smoke_suite");
    let file = project_root.join("src/main.sv");
    let original = fs::read_to_string(&file).expect("smoke suite main source should load");
    let source = original.replace("Entry iVal iVal", "Entry iVal ??");

    let mut compilation = Compilation::new(&project_root);
    compilation.set_overlay(&file, source.clone());

    {
        let result = compilation
            .check(true)
            .expect("first smoke-suite hole check should return diagnostics");
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code == "HOLE"),
            "first check lost the hole diagnostic: {:?}",
            result.diagnostics()
        );
    }

    {
        let result = compilation
            .check(true)
            .expect("second smoke-suite hole check should return diagnostics");
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code == "HOLE"),
            "incremental recheck lost the hole diagnostic: {:?}",
            result.diagnostics()
        );
    }

    let line = "    let intEntries = (map (fun iVal => Entry iVal ??) (rangeInclusive 1 20000)) in";
    let hole_start = offset_in_line(&source, line, "??");
    let hover = compilation
        .hover_info(&file, hole_start)
        .expect("incremental recheck should retain rich hole hover");
    let markdown = hover
        .markdown()
        .expect("incremental recheck should retain diagnostic markdown");
    assert_contains(markdown, "Bindings in scope");
    assert_contains(markdown, "iVal : Int");
    assert_contains(markdown, "Valid fills include");
    assert_contains(markdown, "`iVal`");
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
    assert_eq!(local.signature(), "inner : Int");

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

    let import_list_function = compilation
        .hover_info(
            &main_file,
            offset_in_line(
                &main_source,
                "import hoverLib(documentedIncrement, Documented, MkDocumented, Describe)",
                "documentedIncrement",
            ),
        )
        .expect("selected import function hover should exist");
    assert_eq!(
        import_list_function.signature(),
        "documentedIncrement : Int -> Int"
    );
    assert!(
        import_list_function
            .provenance()
            .is_some_and(|provenance| provenance.contains("hoverLib")),
        "selected import hover should include provenance: {import_list_function:#?}"
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
    assert_eq!(
        imported_type.documentation(),
        Some("Holds a documented integer.")
    );
    assert!(
        imported_type
            .provenance()
            .is_some_and(|provenance| provenance.contains("hoverLib")),
        "imported type hover should include provenance: {imported_type:#?}"
    );

    let imported_constructor = compilation
        .hover_info(
            &main_file,
            offset_in_line(
                &main_source,
                "        MkDocumented n => MkDocumented (documentedIncrement n)",
                "MkDocumented",
            ),
        )
        .expect("imported constructor hover should exist");
    assert_eq!(
        imported_constructor.signature(),
        "MkDocumented : Int -> Documented"
    );
    assert_eq!(
        imported_constructor.documentation(),
        Some("Wraps a documented integer.")
    );

    let operator = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    inner + 1", "+"),
        )
        .expect("operator hover should exist");
    assert_contains(operator.signature(), "class Num a");
    assert_contains(operator.signature(), "(+) :");

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
    assert_eq!(scrutinee.signature(), "value : Documented");

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

    let class_name = compilation
        .hover_info(
            &lib_file,
            offset_in_line(&lib_source, "class Describe a where", "Describe"),
        )
        .expect("class declaration hover should exist");
    assert_eq!(class_name.signature(), "class Describe a");
    assert_eq!(class_name.documentation(), Some("Describes values."));

    let class_predicate = compilation
        .hover_info(
            &main_file,
            offset_in_line_last(
                &main_source,
                "useDescriber : forall a. Describe a => a -> String",
                "Describe",
            ),
        )
        .expect("class predicate hover should exist");
    assert_eq!(class_predicate.signature(), "class Describe a");
    assert!(
        class_predicate
            .provenance()
            .is_some_and(|provenance| provenance.contains("hoverLib")),
        "class predicate hover should include provenance: {class_predicate:#?}"
    );

    let class_method_reference = compilation
        .hover_info(
            &main_file,
            offset_in_line(
                &main_source,
                "useDescriber value = describe value",
                "describe",
            ),
        )
        .expect("class method reference hover should exist");
    assert_contains(class_method_reference.signature(), "class Describe a");
    assert_eq!(
        class_method_reference.signature(),
        "class Describe a\ndescribe : a -> String"
    );

    let prelude_primitive_type = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "localValue : Int", "Int"),
        )
        .expect("primitive prelude type hover should exist");
    assert_eq!(
        prelude_primitive_type.documentation(),
        Some("Whole-number values used for counting, indexing, and integer arithmetic.")
    );

    let prelude_type = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "maybeResult : Maybe Int", "Maybe"),
        )
        .expect("prelude type hover should exist");
    assert_eq!(
        prelude_type.documentation(),
        Some("A value that may be present or absent.")
    );

    let prelude_constructor = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    x <- Just 2", "Just"),
        )
        .expect("prelude constructor hover should exist");
    assert_eq!(
        prelude_constructor.documentation(),
        Some("Wraps a present value.")
    );

    let do_expression = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    pure (x + y)", "pure"),
        )
        .expect("do-block expression hover should exist");
    assert_contains(do_expression.signature(), "Applicative f");
    assert_contains(do_expression.signature(), "pure :");
    assert_eq!(
        do_expression.documentation(),
        Some("Lifts a plain value into a context.")
    );
    assert!(!do_expression.signature().contains("thenM"));
    assert!(!do_expression.signature().contains("bind :"));

    let do_local = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    x <- Just 2", "x"),
        )
        .expect("do-block binder hover should exist");
    assert_eq!(do_local.signature(), "x : Int");

    let do_local_ref = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    pure (x + y)", "x"),
        )
        .expect("do-block local reference hover should exist");
    assert_eq!(do_local_ref.signature(), "x : Int");

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

#[test]
fn hover_formats_inferred_signatures_as_source_text() {
    let project_root = temp_root("inferred_signature");
    let source_path = project_root.join("src/main.sv");
    let source = r#"triple x y z = x + y + z

main : Int
main = triple 1 2 3
"#;
    write_source(&source_path, source);

    let mut compilation = Compilation::new(&project_root);
    let result = compilation.check(true).expect("hover fixture should check");
    assert!(
        result.success(),
        "hover fixture diagnostics: {:?}",
        result.diagnostics()
    );

    let hover = compilation
        .hover_info(
            &source_path,
            offset_in_line(source, "main = triple 1 2 3", "triple"),
        )
        .expect("inferred function hover should exist");
    assert_eq!(
        hover.signature(),
        "triple : forall a. Num a => a -> a -> a -> a"
    );

    drop(project_root);
}

#[test]
fn hover_info_covers_local_declaration_sites() {
    let project_root = fixture_root("tests/fixtures/hover");
    let main_file = project_root.join("src/main.sv");
    let main_source = fs::read_to_string(&main_file).expect("hover fixture source should load");
    let mut compilation = Compilation::new(&project_root);
    let result = compilation.check(true).expect("hover fixture should check");

    assert!(
        result.success(),
        "hover fixture diagnostics: {:?}",
        result.diagnostics()
    );

    let function_param = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "topLevel n = documentedIncrement n", "n"),
        )
        .expect("function parameter declaration hover should exist");
    assert_eq!(function_param.signature(), "n : Int");

    let quiz_root = fixture_root("examples/quiz_game");
    let quiz_file = quiz_root.join("src/main.sv");
    let quiz_source = fs::read_to_string(&quiz_file).expect("quiz source should load");
    let mut quiz = Compilation::new(&quiz_root);
    let result = quiz.check(true).expect("quiz fixture should check");

    assert!(
        result.success(),
        "quiz diagnostics: {:?}",
        result.diagnostics()
    );

    let lambda_line = "statePure value = State (fun state => (value, state))";
    let lambda_offset = {
        let line_start = quiz_source
            .find(lambda_line)
            .expect("quiz source should contain statePure lambda");
        let param_start = lambda_line
            .find("fun state")
            .expect("lambda line should contain `fun state`")
            + "fun ".len();
        line_start + param_start
    };
    let lambda_param = quiz
        .hover_info(&quiz_file, lambda_offset)
        .expect("nested lambda parameter hover should exist");
    assert_eq!(lambda_param.signature(), "state : s");

    let question = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(
                &quiz_source,
                "        (question, answer) => QuizCard question answer",
                "question",
            ),
        )
        .expect("case pattern variable hover should exist");
    assert_eq!(question.signature(), "question : String");

    let answer = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(
                &quiz_source,
                "        (question, answer) => QuizCard question answer",
                "answer",
            ),
        )
        .expect("case pattern variable hover should exist");
    assert_eq!(answer.signature(), "answer : String");

    let ask_cards = quiz
        .hover_info(
            &quiz_file,
            offset_in_line_last(&quiz_source, "askCards cards = mapM askCard cards", "cards"),
        )
        .expect("function parameter reference hover should exist");
    assert_eq!(ask_cards.signature(), "cards : [QuizCard]");
}

#[test]
fn hover_covers_quiz_literals_parens_lists_and_instance_methods() {
    let quiz_root = fixture_root("examples/quiz_game");
    let quiz_file = quiz_root.join("src/main.sv");
    let quiz_source = fs::read_to_string(&quiz_file).expect("quiz source should load");
    let mut quiz = Compilation::new(&quiz_root);
    let result = quiz.check(true).expect("quiz fixture should check");

    assert!(
        result.success(),
        "quiz diagnostics: {:?}",
        result.diagnostics()
    );

    let summary_line =
        r#"    appendString "Score: " (appendString (show score) (appendString "/" (show total)))"#;
    let slash_string = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(&quiz_source, summary_line, r#""/""#) + 1,
        )
        .expect("slash string literal hover should exist");
    assert_eq!(slash_string.signature(), "String");

    let show_score_open = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(&quiz_source, summary_line, "(show score)"),
        )
        .expect("opening paren around show score should hover");
    assert_eq!(show_score_open.signature(), "String");

    let show_score_close = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(&quiz_source, summary_line, "(show score)") + "(show score".len(),
        )
        .expect("closing paren around show score should hover");
    assert_eq!(show_score_close.signature(), "String");

    let prompt = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(&quiz_source, r#"                printStr "> ""#, r#""> ""#) + 1,
        )
        .expect("prompt string literal hover should exist");
    assert_eq!(prompt.signature(), "String");

    let expression_cons = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(
                &quiz_source,
                r#"    if line == "" then rest else line :: rest"#,
                "::",
            ),
        )
        .expect("expression cons hover should exist");
    assert_contains(expression_cons.signature(), "Cons :");
    assert_contains(expression_cons.signature(), "a -> [a] -> [a]");

    let empty_list_open = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(
                &quiz_source,
                "splitLines text = splitLinesChars (stringToChars text) []",
                "[",
            ),
        )
        .expect("empty list opening bracket hover should exist");
    assert_eq!(empty_list_open.signature(), "Nil : forall a. [a]");

    let empty_list_close = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(
                &quiz_source,
                "splitLines text = splitLinesChars (stringToChars text) []",
                "]",
            ),
        )
        .expect("empty list closing bracket hover should exist");
    assert_eq!(empty_list_close.signature(), "Nil : forall a. [a]");

    let pattern_cons = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(&quiz_source, "        | c :: rest =>", "::"),
        )
        .expect("pattern cons hover should exist");
    assert_contains(pattern_cons.signature(), "Cons :");
    assert_contains(pattern_cons.signature(), "a -> [a] -> [a]");

    let instance_class = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(&quiz_source, "instance Functor (State s) where", "Functor"),
        )
        .expect("instance class hover should exist");
    assert_contains(instance_class.signature(), "class Functor f");

    let fmap_method = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(&quiz_source, "    fmap f action =", "fmap"),
        )
        .expect("instance method name hover should exist");
    assert_contains(fmap_method.signature(), "fmap :");
    assert_contains(fmap_method.signature(), "State s");

    let fmap_param = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(&quiz_source, "    fmap f action =", "f action"),
        )
        .expect("instance method parameter hover should exist");
    assert_eq!(fmap_param.signature(), "f : a -> b");

    let fmap_param_ref = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(
                &quiz_source,
                "                (value, nextState) => (f value, nextState)",
                "f",
            ),
        )
        .expect("instance method parameter reference hover should exist");
    assert_eq!(fmap_param_ref.signature(), "f : a -> b");

    let instance_lambda_param = quiz
        .hover_info(
            &quiz_file,
            offset_in_line(&quiz_source, "        State (fun state =>", "state"),
        )
        .expect("nested lambda parameter inside instance method should hover");
    assert_eq!(instance_lambda_param.signature(), "state : s");
}

#[test]
fn hover_on_paren_left_operand_of_cons_uses_group_span() {
    let project_root = temp_root("paren_cons_left_operand_hover");
    let file = project_root.join("src/main.sv");
    let source = r#"import std.list(reverse)
import std.text(charsToString)

data Tok = TAtom String

flushAtom : [Char] -> [Tok] -> [Tok]
flushAtom current tokens =
    case current of
        [] => tokens
        | _ => TAtom (charsToString (reverse current)) :: tokens

main = 0
"#;
    write_source(&file, source);

    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("paren cons hover fixture should check");

    assert!(
        result.success(),
        "paren cons hover diagnostics: {:?}",
        result.diagnostics()
    );

    let line = "        | _ => TAtom (charsToString (reverse current)) :: tokens";
    let hover = compilation
        .hover_info(&file, offset_in_line(source, line, "(charsToString"))
        .expect("opening paren around TAtom argument should hover");

    assert_eq!(hover.signature(), "String");
    assert_eq!(
        source
            .get(hover.span())
            .expect("hover span should slice source"),
        "(charsToString (reverse current))"
    );

    drop(project_root);
}

#[test]
fn hover_covers_pattern_wildcard_types() {
    let scry_root = fixture_root("examples/scry");
    let parser_file = scry_root.join("src/scry/parser.sv");
    let parser_source = fs::read_to_string(&parser_file).expect("scry parser source should load");
    let mut scry = Compilation::new(&scry_root);
    let result = scry.check(true).expect("scry fixture should check");

    assert!(
        result.success(),
        "scry diagnostics: {:?}",
        result.diagnostics()
    );

    for (line, needle, expected) in [
        ("        | _ =>", "_", "_ : [Char]"),
        ("        '?' :: _ => True", "_", "_ : [Char]"),
        ("        | SList _ => Nothing", "_", "_ : [SExp]"),
    ] {
        let hover = scry
            .hover_info(&parser_file, offset_in_line(&parser_source, line, needle))
            .unwrap_or_else(|| panic!("wildcard in `{line}` should hover"));
        assert_eq!(hover.signature(), expected);
    }
}

#[test]
fn hover_covers_non_empty_and_nested_list_literal_brackets() {
    let smoke_root = fixture_root("examples/smoke_suite");
    let cool_file = smoke_root.join("src/tcTests/coolStuff.sv");
    let cool_source = fs::read_to_string(&cool_file).expect("coolStuff source should load");
    let mut smoke = Compilation::new(&smoke_root);
    let result = smoke.check(true).expect("smoke suite should check");

    assert!(
        result.success(),
        "smoke diagnostics: {:?}",
        result.diagnostics()
    );

    let outer_line = "    eq allSubsets3 [";
    let outer_open = smoke
        .hover_info(&cool_file, offset_in_line(&cool_source, outer_line, "["))
        .expect("outer list opening bracket should hover");
    assert_eq!(outer_open.signature(), "[[Int]]");

    let inner_line = "        [1, 2, 3],";
    let inner_open = smoke
        .hover_info(&cool_file, offset_in_line(&cool_source, inner_line, "["))
        .expect("inner list opening bracket should hover");
    assert_eq!(inner_open.signature(), "[Int]");

    let inner_close = smoke
        .hover_info(&cool_file, offset_in_line(&cool_source, inner_line, "]"))
        .expect("inner list closing bracket should hover");
    assert_eq!(inner_close.signature(), "[Int]");
}

#[test]
fn hover_covers_direct_prelude_imported_references() {
    let workspace_root = fixture_root(".");
    let typeclasses_file = workspace_root.join("lib/prelude/src/prelude/typeclasses.sv");
    let typeclasses_source =
        fs::read_to_string(&typeclasses_file).expect("typeclasses source should load");
    let mut compilation = Compilation::for_source_file(&typeclasses_file);
    let result = compilation
        .check(true)
        .expect("direct typeclasses check should finish");

    assert!(
        result.success(),
        "direct typeclasses diagnostics: {:?}",
        result.diagnostics()
    );

    for (line, needle, expected) in [
        ("class Eq a where", "a where", "a : *"),
        ("    (==) : a -> a -> Bool", "a", "a : *"),
        ("class Functor f where", "f", "f : * -> *"),
        (
            "    fmap : forall a b. (a -> b) -> f a -> f b",
            "f a",
            "f : * -> *",
        ),
        (
            "    fmap : forall a b. (a -> b) -> f a -> f b",
            "a ->",
            "a : *",
        ),
        (
            "class Functor f => Applicative f where",
            "f =>",
            "f : * -> *",
        ),
        (
            "class Functor f => Applicative f where",
            "f where",
            "f : * -> *",
        ),
        (
            "mconcat xs = foldr combine empty xs",
            "foldr",
            "foldr : forall a b. (a -> b -> b) -> b -> [a] -> b",
        ),
    ] {
        let hover = compilation
            .hover_info(
                &typeclasses_file,
                offset_in_line(&typeclasses_source, line, needle),
            )
            .unwrap_or_else(|| panic!("`{needle}` in `{line}` should hover"));
        assert_eq!(hover.signature(), expected);
    }

    let instances_file = workspace_root.join("lib/prelude/src/prelude/instances.sv");
    let instances_source =
        fs::read_to_string(&instances_file).expect("instances source should load");
    let mut instances_compilation = Compilation::for_source_file(&instances_file);
    let instances_result = instances_compilation
        .check(true)
        .expect("direct instances check should finish");
    assert!(
        instances_result.success(),
        "direct instances diagnostics: {:?}",
        instances_result.diagnostics()
    );

    for (line, needle, expected) in [
        ("instance Eq a => Eq (List a) where", "a =>", "a : *"),
        ("instance Eq a => Eq (List a) where", "a)", "a : *"),
        ("instance Functor ((,) e) where", "e)", "e : *"),
        (
            "    compare x y = intCompare x y",
            "intCompare",
            "intCompare : Int -> Int -> Ordering",
        ),
        (
            "    compare x y = charCompare x y",
            "charCompare",
            "charCompare : Char -> Char -> Ordering",
        ),
        (
            "    compare x y = stringCompare x y",
            "stringCompare",
            "stringCompare : String -> String -> Ordering",
        ),
        (
            "    apply fs xs = concat (map (fun f => map f xs) fs)",
            "map",
            "map : forall a b. (a -> b) -> [a] -> [b]",
        ),
    ] {
        let hover = instances_compilation
            .hover_info(
                &instances_file,
                offset_in_line(&instances_source, line, needle),
            )
            .unwrap_or_else(|| panic!("`{needle}` in `{line}` should hover"));
        assert_eq!(hover.signature(), expected);
    }
}

#[test]
fn hover_audits_quiz_game_sensitive_tokens() {
    let quiz_root = fixture_root("examples/quiz_game");
    let quiz_file = quiz_root.join("src/main.sv");
    let quiz_source = fs::read_to_string(&quiz_file).expect("quiz source should load");
    let mut quiz = Compilation::new(&quiz_root);
    let result = quiz.check(true).expect("quiz fixture should check");

    assert!(
        result.success(),
        "quiz diagnostics: {:?}",
        result.diagnostics()
    );

    let actual = hover_audit_for_lines(
        &mut quiz,
        &quiz_file,
        &quiz_source,
        &[
            "statePure value = State (fun state => (value, state))",
            "askCards cards = mapM askCard cards",
            "        (question, answer) => QuizCard question answer",
            "        responses <- askCards cards",
        ],
    );
    assert!(!actual.contains("thenM"));
    assert!(!actual.contains("bind :"));

    let expected = concat!(
        " 15:1   `statePure` => statePure : forall s a. a -> State s a\n",
        " 15:11  `value` => value : a\n",
        " 15:17  `=` => <none>\n",
        " 15:19  `State` => State : forall s a. (s -> (a, s)) -> State s a\n",
        " 15:25  `(` => s -> (a, s)\n",
        " 15:26  `fun` => <none>\n",
        " 15:30  `state` => state : s\n",
        " 15:36  `=>` => <none>\n",
        " 15:39  `(` => (a, s)\n",
        " 15:40  `value` => value : a\n",
        " 15:45  `,` => <none>\n",
        " 15:47  `state` => state : s\n",
        " 15:52  `)` => (a, s)\n",
        " 15:53  `)` => s -> (a, s)\n",
        " 76:9   `(` => (String, String)\n",
        " 76:10  `question` => question : String\n",
        " 76:18  `,` => <none>\n",
        " 76:20  `answer` => answer : String\n",
        " 76:26  `)` => (String, String)\n",
        " 76:28  `=>` => <none>\n",
        " 76:31  `QuizCard` => QuizCard : String -> String -> QuizCard\n",
        " 76:40  `question` => question : String\n",
        " 76:49  `answer` => answer : String\n",
        " 92:1   `askCards` => askCards : [QuizCard] -> Task [(QuizCard, String)]\n",
        " 92:10  `cards` => cards : [QuizCard]\n",
        " 92:16  `=` => <none>\n",
        " 92:18  `mapM` => mapM : forall m a b. Monad m => (a -> m b) -> [a] -> m [b]\n",
        " 92:23  `askCard` => askCard : QuizCard -> Task (QuizCard, String)\n",
        " 92:31  `cards` => cards : [QuizCard]\n",
        "138:9   `responses` => responses : [(QuizCard, String)]\n",
        "138:19  `<-` => <none>\n",
        "138:22  `askCards` => askCards : [QuizCard] -> Task [(QuizCard, String)]\n",
        "138:31  `cards` => cards : [QuizCard]\n",
    );

    assert_eq!(actual, expected);
}
