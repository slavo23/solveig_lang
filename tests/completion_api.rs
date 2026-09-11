#![expect(
    clippy::panic,
    reason = "integration tests use panic messages to explain fixture lookup failures"
)]

use solveig_base::{PositionEncoding, SourcePosition};
use solveig_driver::Compilation;
use solveig_ide::{CompilationIdeExt, SourceCompletionItem, SourceCompletionItemKind};
use std::fs;
use std::path::Path;

const MARKER: &str = "$COMPLETE";

use crate::support;

fn temp_root(name: &str) -> support::TestRoot {
    support::temp_root("solveig_completion", name)
}

fn write_source(path: &Path, source: &str) {
    fs::create_dir_all(
        path.parent()
            .expect("test source should have a parent directory"),
    )
    .expect("test source directory should be creatable");
    fs::write(path, source).expect("test source should be writable");
}

fn source_without_marker(source: &str) -> (String, SourcePosition) {
    let marker_offset = source
        .find(MARKER)
        .expect("test source should include a completion marker");
    let prefix = source
        .get(..marker_offset)
        .expect("completion marker offset should be a UTF-8 boundary");
    let line = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count())
        .expect("test line number should fit in u32");
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let character =
        u32::try_from(prefix.len() - line_start).expect("test character should fit in u32");

    (
        source.replacen(MARKER, "", 1),
        SourcePosition::new(line, character),
    )
}

fn completion_items(source: &str) -> Vec<SourceCompletionItem> {
    let root = temp_root("api");
    let source_path = root.join("scratch.sv");
    let (source, position) = source_without_marker(source);

    write_source(&source_path, &source);
    let mut compilation = Compilation::for_source_file(&source_path);
    let items =
        compilation.completion_items_at_position(&source_path, position, PositionEncoding::Utf8);

    drop(root);
    items
}

fn package_completion_items(test_name: &str, source: &str) -> Vec<SourceCompletionItem> {
    let root = temp_root(test_name);
    let source_path = root.join("src/main.sv");
    let (source, position) = source_without_marker(source);

    write_source(
        &root.join("solveig.toml"),
        "[package]\nname = \"completion_pkg\"\nversion = \"0.1.0\"\n\n[language]\nentry = \"main\"\n",
    );
    write_source(
        &root.join("src/core.sv"),
        r#"export (Widget(..), Render, value, other)

data Widget = MkWidget Int

class Render a where
    render : a -> String

value = 1
other = 2
"#,
    );
    write_source(&source_path, &source);

    let mut compilation = Compilation::new(&root);
    let items =
        compilation.completion_items_at_position(&source_path, position, PositionEncoding::Utf8);

    drop(root);
    items
}

fn find_item<'a>(
    items: &'a [SourceCompletionItem],
    label: &str,
) -> Option<&'a SourceCompletionItem> {
    items.iter().find(|item| item.label() == label)
}

fn assert_item(items: &[SourceCompletionItem], label: &str, kind: SourceCompletionItemKind) {
    let item = find_item(items, label)
        .unwrap_or_else(|| panic!("missing completion `{label}` in {items:#?}"));
    assert_eq!(item.kind(), kind);
}

#[test]
fn completion_lists_parameters_toplevels_methods_constructors_and_keywords() {
    let items = completion_items(
        r#"-- | The boxed type.
data Box = MkBox Int

class Describe a where
    describe : a -> String

-- | Adds one to a value.
increment : Int -> Int
increment value = $COMPLETEvalue

main : Int
main = increment 1
"#,
    );

    assert_item(&items, "value", SourceCompletionItemKind::Variable);
    assert_item(&items, "increment", SourceCompletionItemKind::Function);
    assert_item(&items, "Box", SourceCompletionItemKind::Type);
    assert_item(&items, "MkBox", SourceCompletionItemKind::Constructor);
    assert_item(&items, "Describe", SourceCompletionItemKind::Class);
    assert_item(&items, "describe", SourceCompletionItemKind::Method);
    assert_item(&items, "if", SourceCompletionItemKind::Keyword);

    let increment = find_item(&items, "increment").expect("increment completion should exist");
    assert!(
        increment
            .detail()
            .is_some_and(|detail| detail.contains("Int -> Int"))
    );
    assert_eq!(increment.documentation(), Some("Adds one to a value."));
}

#[test]
fn qualified_prelude_completion_steps_through_modules_then_exports() {
    let modules = completion_items(
        r#"main : Bool
main = prelude.$COMPLETEbuiltins.True
"#,
    );

    assert_item(&modules, "builtins", SourceCompletionItemKind::Module);
    assert!(find_item(&modules, "Bool").is_none());

    let exports = completion_items(
        r#"main : Bool
main = prelude.builtins.$COMPLETETrue
"#,
    );
    assert_item(&exports, "Bool", SourceCompletionItemKind::Type);
    assert_item(&exports, "True", SourceCompletionItemKind::Constructor);
}

#[test]
fn header_completion_steps_through_modules_and_imported_exports() {
    let modules = completion_items(
        r#"import std.$COMPLETE

main : Int
main = 1
"#,
    );

    assert_item(&modules, "channel", SourceCompletionItemKind::Module);
    assert_item(&modules, "http", SourceCompletionItemKind::Module);
    assert!(find_item(&modules, "send").is_none());

    let exports = completion_items(
        r#"import std.channel($COMPLETE)

main : Int
main = 1
"#,
    );

    assert_item(&exports, "newChan", SourceCompletionItemKind::Function);
    assert_item(&exports, "send", SourceCompletionItemKind::Function);
    assert!(find_item(&exports, "if").is_none());
    assert!(find_item(&exports, "readFile").is_none());

    let partial = completion_items(
        r#"import std.channel(se$COMPLETE)

main : Int
main = 1
"#,
    );
    assert_item(&partial, "send", SourceCompletionItemKind::Function);
    assert!(find_item(&partial, "readFile").is_none());

    let remaining = completion_items(
        r#"import std.channel(send, $COMPLETE)

main : Int
main = 1
"#,
    );
    assert_item(&remaining, "recv", SourceCompletionItemKind::Function);
    assert!(find_item(&remaining, "send").is_none());
}

#[test]
fn module_selection_completion_uses_the_correct_visibility_projection() {
    let exports = completion_items(
        r#"export ($COMPLETE)

data Widget = MkWidget Int

class Render a where
    render : a -> String

answer : Int
answer = 42
"#,
    );

    assert_item(&exports, "Widget", SourceCompletionItemKind::Type);
    assert_item(&exports, "Render", SourceCompletionItemKind::Class);
    assert!(find_item(&exports, "MkWidget").is_none());
    assert!(find_item(&exports, "render").is_none());
    assert_item(&exports, "answer", SourceCompletionItemKind::Function);
    assert!(find_item(&exports, "map").is_none());
    assert!(find_item(&exports, "if").is_none());

    let private = completion_items(
        r#"private ($COMPLETE)

data Widget = MkWidget Int

class Render a where
    render : a -> String

answer : Int
answer = 42
"#,
    );
    assert_item(&private, "Widget", SourceCompletionItemKind::Type);
    assert_item(&private, "Render", SourceCompletionItemKind::Class);
    assert_item(&private, "answer", SourceCompletionItemKind::Function);
    assert!(find_item(&private, "MkWidget").is_none());
    assert!(find_item(&private, "render").is_none());
    assert!(find_item(&private, "map").is_none());

    let partial = completion_items(
        r#"export (an$COMPLETE)

answer : Int
answer = 42
"#,
    );
    assert_item(&partial, "answer", SourceCompletionItemKind::Function);
    assert!(find_item(&partial, "map").is_none());

    let reexports = completion_items(
        r#"reexport std.channel($COMPLETE)

main : Int
main = 1
"#,
    );

    assert_item(
        &reexports,
        "newBoundedChan",
        SourceCompletionItemKind::Function,
    );
    assert_item(&reexports, "recv", SourceCompletionItemKind::Function);
    assert!(find_item(&reexports, "readFile").is_none());
}

#[test]
fn project_completion_respects_import_selection_qualification_and_aliases() {
    let header = package_completion_items(
        "selectable_import_entities",
        r#"import completion_pkg.core($COMPLETE)

main = 1
"#,
    );
    assert_item(&header, "Widget", SourceCompletionItemKind::Type);
    assert_item(&header, "Render", SourceCompletionItemKind::Class);
    assert_item(&header, "value", SourceCompletionItemKind::Function);
    assert!(find_item(&header, "MkWidget").is_none());
    assert!(find_item(&header, "render").is_none());

    let unqualified = package_completion_items(
        "selected_imports",
        r#"import completion_pkg.core(value as answer, Widget)

main = $COMPLETEanswer
"#,
    );
    assert_item(&unqualified, "answer", SourceCompletionItemKind::Function);
    assert_item(&unqualified, "Widget", SourceCompletionItemKind::Type);
    for hidden in ["value", "other", "MkWidget"] {
        assert!(
            find_item(&unqualified, hidden).is_none(),
            "selection leaked `{hidden}` into unqualified completion: {unqualified:#?}"
        );
    }

    let qualified = package_completion_items(
        "qualified_imports",
        r#"import completion_pkg.core(value as answer, Widget)

main = core.$COMPLETEvalue
"#,
    );
    assert_item(&qualified, "value", SourceCompletionItemKind::Function);
    assert_item(&qualified, "other", SourceCompletionItemKind::Function);
    assert_item(&qualified, "Widget", SourceCompletionItemKind::Type);
    assert_item(
        &qualified,
        "MkWidget",
        SourceCompletionItemKind::Constructor,
    );
    assert!(find_item(&qualified, "answer").is_none());

    let alias = package_completion_items(
        "aliased_imports",
        r#"import completion_pkg.core as Model

main = Model.$COMPLETEvalue
"#,
    );
    assert_item(&alias, "value", SourceCompletionItemKind::Function);
    assert_item(&alias, "other", SourceCompletionItemKind::Function);

    let suppressed = package_completion_items(
        "suppressed_import_qualifier",
        r#"import completion_pkg.core as Model

main = core.$COMPLETEvalue
"#,
    );
    assert!(
        suppressed.is_empty(),
        "an explicit alias must suppress the original qualifier: {suppressed:#?}"
    );
}

#[test]
fn do_block_completion_lists_previous_binders() {
    let items = completion_items(
        r#"main : Maybe Int
main = do
    first <- Just 1
    second <- Just 2
    pure ($COMPLETEfirst + second)
"#,
    );

    assert_item(&items, "first", SourceCompletionItemKind::Variable);
    assert_item(&items, "second", SourceCompletionItemKind::Variable);
}

#[test]
fn completion_lists_every_name_from_local_patterns() {
    let items = completion_items(
        r#"data User = User String Int
data Box a = Box a

main : Maybe Int
main = do
    (label, Box amount) <- Just ("total", Box 7)
    let User name age = User label amount
    pure ($COMPLETEamount + age)
"#,
    );

    for name in ["label", "amount", "name", "age"] {
        assert_item(&items, name, SourceCompletionItemKind::Variable);
    }
}

#[test]
fn completion_survives_mid_edit_parse_errors() {
    let items = completion_items(
        r#"main : Int
main = if True then $COMPLETE
"#,
    );

    assert_item(&items, "else", SourceCompletionItemKind::Keyword);
    assert_item(&items, "then", SourceCompletionItemKind::Keyword);
}

#[test]
fn completion_keeps_function_parameters_visible_at_the_typed_prefix_end() {
    let items = completion_items(
        r#"add x1 = x$COMPLETE
"#,
    );

    assert_item(&items, "x1", SourceCompletionItemKind::Variable);
}
