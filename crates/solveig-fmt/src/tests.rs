use solveig_base::Session;
use solveig_syntax::lowering::parse_source;

use super::{format, format_subtree};

fn format_source(source: &str, width: usize) -> String {
    let session = Session::new();
    let file = session.add_file("format.sv", source);
    let parsed = parse_source(&session, file, source);
    assert!(
        session.diagnostics().is_empty(),
        "formatter fixture must parse without diagnostics"
    );
    let formatted = format(&parsed.cst, source, width);

    let verification_session = Session::new();
    let verification_file = verification_session.add_file("formatted.sv", &formatted);
    let reparsed = parse_source(&verification_session, verification_file, &formatted);
    assert!(
        verification_session.diagnostics().is_empty(),
        "formatted fixture must remain parseable:\n{formatted}"
    );
    assert_eq!(
        format(&reparsed.cst, &formatted, width),
        formatted,
        "focused formatter goldens must already be idempotent"
    );

    formatted
}

#[test]
fn formats_signatures_with_forall_and_contexts() {
    insta::assert_snapshot!(format_source(
        "render:forall a.(Show a,Eq a)=>a->String\n",
        100,
    ));
}

#[test]
fn aligns_every_broken_function_type_at_one_continuation_indent() {
    insta::assert_snapshot!(format_source(
        "runPool:(String->Int->String->String->Task (Maybe Bytes))->String->Int->String->(Chan DownloadJob->Task ())->Task ()\n",
        76,
    ));
}

#[test]
fn formats_function_definitions_and_application_chains() {
    insta::assert_snapshot!(format_source(
        "greet   name=appendString   \"hello, \" name\n",
        30,
    ));
}

#[test]
fn formats_data_declarations_and_constructors() {
    insta::assert_snapshot!(format_source(
        "data Tree a=Leaf a|Branch (Tree a) (Tree a)\n",
        32,
    ));
}

#[test]
fn formats_class_and_instance_bodies() {
    insta::assert_snapshot!(format_source(
        "class Eq a where\n eq:a->a->Bool\n default eq x y=True\ninstance Eq Int where\n eq 0 0=True\n eq _ _=False\n",
        100,
    ));
}

#[test]
fn formats_let_if_and_lambda_expressions() {
    insta::assert_snapshot!(format_source(
        "choose x=let y=fun value=>value in if x then y 1 else y 2\n",
        36,
    ));
}

#[test]
fn formats_case_expressions_with_nested_patterns() {
    insta::assert_snapshot!(format_source(
        "data User=User String String\nemail user=case user of\n User name address=>address\n | _=>\"unknown\"\n",
        100,
    ));
}

#[test]
fn formats_do_blocks_one_statement_per_line() {
    insta::assert_snapshot!(format_source(
        "main=do\n name<-readLine\n let greeting=appendString \"hello, \" name\n printLine greeting\n",
        100,
    ));
}

#[test]
fn formats_nested_do_blocks_with_normalized_indentation() {
    insta::assert_snapshot!(format_source(
        "nested=do\n first<-do\n      x<-Just 1\n      pure x\n second<-do\n      y<-Just 2\n      pure y\n pure(first+second)\nstandalone=do\n do\n      printLine \"nested\"\n      pure ()\n",
        100,
    ));
}

#[test]
fn splits_explicit_do_semicolons_into_statement_lines() {
    insta::assert_snapshot!(format_source(
        "main=do printLine \"one\";printLine \"two\";pure ()\n",
        100,
    ));
}

#[test]
fn splits_explicit_class_and_instance_semicolons_into_declaration_lines() {
    insta::assert_snapshot!(format_source(
        "class Render a where render:a->String;default render x=\"value\"\ninstance Render Int where render 0=\"zero\";render _=\"other\"\n",
        100,
    ));
}

#[test]
fn preserves_comments_at_do_statement_boundaries() {
    insta::assert_snapshot!(format_source(
        "main=do\n -- before first\n printLine \"one\" -- after first\n -- before second\n printLine \"two\"\n",
        100,
    ));
}

#[test]
fn formats_mixed_operator_chains() {
    insta::assert_snapshot!(format_source("main=1+2*3==7\n", 12));
}

#[test]
fn formats_parentheses_lists_tuples_sections_and_backticks() {
    insta::assert_snapshot!(format_source(
        "mix x y=(x,[y,2],(1+),(`mix` y),1 `mix` y)\n",
        28,
    ));
}

#[test]
fn broken_nested_delimiters_indent_contents_and_place_closers_consistently() {
    insta::assert_snapshot!(format_source(
        "nested value=(value,(appendString \"a long prefix\" value),[value,appendString \"another prefix\" value])\n",
        44,
    ));
}

#[test]
fn formats_imports_fixities_and_type_aliases() {
    insta::assert_snapshot!(format_source(
        "import std.http as Http(fetch,Response)\ninfixl 5 +++,`append`\ntype Handler e a=IO e (Result e a)\n",
        40,
    ));
}

#[test]
fn separates_imports_without_detaching_declaration_comments() {
    insta::assert_snapshot!(format_source(
        "import std.io\nimport std.list\n-- | Runs the program.\nmain:Task ()\nmain=pure ()\n",
        100,
    ));
}

#[test]
fn keeps_trailing_import_comments_with_the_import_block() {
    assert_eq!(
        format_source(
            "import std.io -- required by main\nmain:Task ()\nmain=pure ()\n",
            100,
        ),
        "import std.io -- required by main\n\nmain : Task ()\nmain = pure ()\n"
    );
}

#[test]
fn emits_comments_once_and_collapses_blank_line_runs() {
    insta::assert_snapshot!(format_source(
        "-- | identity\nid x=x\n\n\n-- ordinary\nmain=id 1\n",
        100,
    ));
}

#[test]
fn formats_one_definition_without_consuming_the_next_definition() {
    let source = "-- | docs\nfirst   x=x+1 -- result\n\nsecond   y=y\n";
    let session = Session::new();
    let file = session.add_file("format.sv", source);
    let parsed = parse_source(&session, file, source);
    assert!(session.diagnostics().is_empty());

    let definition = parsed.defs[0].2;
    let (range, formatted) =
        format_subtree(&parsed.cst, source, definition, 100).expect("definition has source text");

    let original = source
        .get(range)
        .expect("formatter-owned range should slice its source");
    assert_eq!(original, "first   x=x+1");
    assert_eq!(formatted, "first x = x + 1");
    assert_eq!(
        source.replacen(original, &formatted, 1),
        "-- | docs\nfirst x = x + 1 -- result\n\nsecond   y=y\n"
    );
}

#[test]
fn formats_attached_constructor_and_method_comments() {
    insta::assert_snapshot!(format_source(
        "-- | choice docs\ndata Choice=\n -- | first choice\n First\n | -- | second choice\n Second\n\n-- | class docs\nclass Describe a where\n -- | method docs\n describe:a->String\n",
        100,
    ));
}
