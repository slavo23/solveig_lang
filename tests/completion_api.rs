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

mod support;

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
fn qualified_prelude_completion_lists_imported_exports() {
    let items = completion_items(
        r#"main : Bool
main = prelude.$COMPLETEbuiltins.True
"#,
    );

    assert_item(&items, "builtins", SourceCompletionItemKind::Module);
    assert_item(&items, "Bool", SourceCompletionItemKind::Type);
    assert_item(&items, "True", SourceCompletionItemKind::Constructor);
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
