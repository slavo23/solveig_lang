#![expect(
    clippy::panic,
    reason = "integration tests use panic messages to explain fixture lookup failures"
)]

use solveig_base::{FileSpan, PositionEncoding, SourceMap, SourcePosition};
use solveig_driver::Compilation;
use solveig_ide::{CompilationIdeExt, SourceDocumentHighlightKind, SourceFoldingRangeKind};
use std::fs;
use std::path::Path;

use crate::support;

fn temp_root(name: &str) -> support::TestRoot {
    support::temp_root("solveig_editor_protocol", name)
}

fn write_source(path: &Path, source: &str) {
    fs::create_dir_all(
        path.parent()
            .expect("test source should have a parent directory"),
    )
    .expect("test source directory should be creatable");
    fs::write(path, source).expect("test source should be writable");
}

fn position_in_line(source: &str, line: &str, needle: &str) -> SourcePosition {
    let line_start = source
        .find(line)
        .unwrap_or_else(|| panic!("source should contain line `{line}`"));
    let needle_offset = line
        .find(needle)
        .unwrap_or_else(|| panic!("line `{line}` should contain `{needle}`"));
    let line_number = source
        .get(..line_start)
        .expect("test line start should be a UTF-8 boundary")
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count();

    SourcePosition::new(
        u32::try_from(line_number).expect("test line number should fit in u32"),
        u32::try_from(needle_offset).expect("test character offset should fit in u32"),
    )
}

fn position_in_line_last(source: &str, line: &str, needle: &str) -> SourcePosition {
    let line_start = source
        .find(line)
        .unwrap_or_else(|| panic!("source should contain line `{line}`"));
    let needle_offset = line
        .rfind(needle)
        .unwrap_or_else(|| panic!("line `{line}` should contain `{needle}`"));
    let line_number = source
        .get(..line_start)
        .expect("test line start should be a UTF-8 boundary")
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count();

    SourcePosition::new(
        u32::try_from(line_number).expect("test line number should fit in u32"),
        u32::try_from(needle_offset).expect("test character offset should fit in u32"),
    )
}

fn source_text(source_map: &SourceMap, span: FileSpan) -> &str {
    source_map
        .lookup(span.file)
        .src
        .get(span.span.to_usize_range())
        .expect("span should slice retained source")
}

#[test]
fn formatting_queries_return_whole_document_and_structural_range_edits() {
    let project_root = temp_root("formatting_queries");
    let source_path = project_root.join("Main.sv");
    let source = "-- | first docs\nfirst   x=x+1\n\nsecond   y=y\n";
    write_source(&source_path, source);

    let mut compilation = Compilation::for_source_file(&source_path);
    let document_edits = compilation
        .format_document(&source_path, 100)
        .expect("standalone local source should be editable");
    assert_eq!(document_edits.len(), 1);
    assert_eq!(document_edits[0].span().to_usize_range(), 0..source.len());
    assert_eq!(
        document_edits[0].new_text(),
        "-- | first docs\nfirst x = x + 1\n\nsecond y = y\n"
    );

    let first_edits = compilation
        .format_range(
            &source_path,
            SourcePosition::new(1, 2),
            SourcePosition::new(1, 8),
            PositionEncoding::Utf8,
            100,
        )
        .expect("the first definition should be formatable");
    assert_eq!(first_edits.len(), 1);
    assert_eq!(
        source_text(compilation.source_map(), first_edits[0].file_span()),
        "first   x=x+1"
    );
    assert_eq!(first_edits[0].new_text(), "first x = x + 1");

    let crossing_edits = compilation
        .format_range(
            &source_path,
            SourcePosition::new(1, 0),
            SourcePosition::new(3, 5),
            PositionEncoding::Utf8,
            100,
        )
        .expect("a local range request should remain valid");
    assert!(
        crossing_edits.is_empty(),
        "a range touching two definitions must not be formatted"
    );

    drop(project_root);
}

#[test]
fn formatting_parse_errors_returns_no_edits() {
    let project_root = temp_root("formatting_parse_error");
    let source_path = project_root.join("Main.sv");
    write_source(&source_path, "broken :\nmain=1\n");

    let mut compilation = Compilation::for_source_file(&source_path);
    assert!(
        compilation
            .format_document(&source_path, 100)
            .expect("the source is local")
            .is_empty(),
        "formatting must not edit a CST containing a parser error"
    );

    drop(project_root);
}

fn signature_label_at(
    compilation: &mut Compilation,
    file: &Path,
    source: &str,
    line: &str,
    needle: &str,
) -> (String, Vec<String>, u32, Option<String>) {
    let help = compilation
        .signature_help_at_position(
            file,
            position_in_line(source, line, needle),
            PositionEncoding::Utf8,
        )
        .unwrap_or_else(|| panic!("signature help should exist at `{needle}` in `{line}`"));
    let signature = help
        .signatures()
        .first()
        .expect("signature help should include one signature");

    (
        signature.label().to_string(),
        signature.parameters().to_vec(),
        help.active_parameter(),
        signature.documentation().map(str::to_string),
    )
}

fn signature_label_at_last(
    compilation: &mut Compilation,
    file: &Path,
    source: &str,
    line: &str,
    needle: &str,
) -> (String, Vec<String>, u32, Option<String>) {
    let help = compilation
        .signature_help_at_position(
            file,
            position_in_line_last(source, line, needle),
            PositionEncoding::Utf8,
        )
        .unwrap_or_else(|| panic!("signature help should exist at last `{needle}` in `{line}`"));
    let signature = help
        .signatures()
        .first()
        .expect("signature help should include one signature");

    (
        signature.label().to_string(),
        signature.parameters().to_vec(),
        help.active_parameter(),
        signature.documentation().map(str::to_string),
    )
}

#[test]
fn editor_protocol_features_cover_remaining_lsp_payloads() {
    let project_root = temp_root("api");
    let main = project_root.join("src/main.sv");
    let helper = project_root.join("src/helper.sv");
    let main_source = r#"import helper(increment)

-- fold comment one
-- fold comment two
main : Int
main =
    increment 41

choose : Bool -> Int
choose flag =
    case flag of
        True => increment 1
        False => 0

maybeMain : Maybe Int
maybeMain = do
    let local = 41
    pure local
"#;
    let helper_source = r#"-- | Increments a number.
increment : Int -> Int
increment value = value + 1
"#;
    write_source(&main, main_source);
    write_source(&helper, helper_source);
    let mut compilation = Compilation::new(&project_root);
    let result = compilation.check(true).expect("check should finish");

    assert!(result.success(), "{:#?}", result.diagnostics());

    let increment_position = position_in_line(main_source, "    increment 41", "increment");
    let highlights = compilation.document_highlights_at_position(
        &main,
        increment_position,
        PositionEncoding::Utf8,
    );
    assert!(highlights.len() >= 2, "{highlights:#?}");
    assert!(
        highlights
            .iter()
            .any(|highlight| highlight.kind() == SourceDocumentHighlightKind::Read)
    );
    assert!(highlights.iter().all(|highlight| {
        compilation
            .source_map()
            .lookup(highlight.file_span().file)
            .path
            == main
            && source_text(compilation.source_map(), highlight.file_span()) == "increment"
    }));

    let folding_ranges = compilation.folding_ranges(&main);
    assert!(
        folding_ranges
            .iter()
            .any(|range| range.kind() == SourceFoldingRangeKind::Comment)
    );
    assert!(folding_ranges.iter().any(|range| {
        range.kind() == SourceFoldingRangeKind::Region
            && source_text(compilation.source_map(), range.file_span()).contains("case flag of")
    }));
    assert!(folding_ranges.iter().any(|range| {
        range.kind() == SourceFoldingRangeKind::Region
            && source_text(compilation.source_map(), range.file_span()).contains("do\n")
    }));

    let selection_ranges =
        compilation.selection_ranges(&main, &[increment_position], PositionEncoding::Utf8);
    assert_eq!(selection_ranges.len(), 1);
    assert_eq!(
        source_text(compilation.source_map(), selection_ranges[0].file_span()),
        "increment"
    );
    assert!(
        selection_ranges[0]
            .parent()
            .and_then(|parent| parent.parent())
            .is_some()
    );

    let argument_position = position_in_line(main_source, "    increment 41", "41");
    let signature_help = compilation
        .signature_help_at_position(&main, argument_position, PositionEncoding::Utf8)
        .expect("signature help should exist for function application");
    let signature = signature_help
        .signatures()
        .first()
        .expect("signature help should include one signature");
    assert!(signature.label().contains("increment : Int -> Int"));
    assert_eq!(signature.parameters(), ["Int"]);
    assert_eq!(signature_help.active_parameter(), 0);

    let rename_span = compilation
        .prepare_rename_at_position(&main, increment_position, PositionEncoding::Utf8)
        .expect("local source should be editable")
        .expect("increment should be renameable");
    assert_eq!(
        source_text(compilation.source_map(), rename_span),
        "increment"
    );
    let edits = compilation
        .rename_at_position(&main, increment_position, PositionEncoding::Utf8, "bump")
        .expect("local rename should be editable");
    assert!(edits.len() >= 4, "{edits:#?}");
    assert!(edits.iter().all(|edit| edit.new_text() == "bump"));
    assert!(
        edits
            .iter()
            .all(|edit| { source_text(compilation.source_map(), edit.file_span()) == "increment" })
    );
    assert!(
        edits
            .iter()
            .any(|edit| compilation.source_map().lookup(edit.file()).path == main)
    );
    assert!(
        edits
            .iter()
            .any(|edit| compilation.source_map().lookup(edit.file()).path == helper)
    );

    let links = compilation.document_links(&main);
    let link = links
        .iter()
        .find(|link| link.target().ends_with("helper.sv"))
        .unwrap_or_else(|| panic!("missing helper document link: {links:#?}"));
    assert_eq!(
        source_text(compilation.source_map(), link.file_span()),
        "helper"
    );
    assert_eq!(link.tooltip(), Some("Open module helper"));

    drop(project_root);
}

#[test]
fn signature_help_uses_position_index_for_nested_and_desugared_contexts() {
    let project_root = temp_root("signature_index");
    let main = project_root.join("src/main.sv");
    let source = r#"data Packet = Packet Int String

nested : String
nested = appendString (appendString "a" "b") "c"

mkPacket : Packet
mkPacket = Packet 1 "ok"

consed : [Int] -> [Int]
consed tail = 1 :: tail

helpText : String
helpText = "help"

main : Task ()
main =
    do
        printStr helpText
        pure ()
"#;
    write_source(&main, source);
    let mut compilation = Compilation::new(&project_root);
    let result = compilation.check(true).expect("check should finish");

    assert!(result.success(), "{:#?}", result.diagnostics());

    let (inner_label, inner_params, inner_active, _) = signature_label_at(
        &mut compilation,
        &main,
        source,
        r#"nested = appendString (appendString "a" "b") "c""#,
        r#""b""#,
    );
    assert!(
        inner_label.contains("appendString : String -> String -> String"),
        "{inner_label}"
    );
    assert_eq!(inner_params, ["String", "String"]);
    assert_eq!(inner_active, 1);

    let (outer_label, outer_params, outer_active, _) = signature_label_at(
        &mut compilation,
        &main,
        source,
        r#"nested = appendString (appendString "a" "b") "c""#,
        r#""c""#,
    );
    assert!(
        outer_label.contains("appendString : String -> String -> String"),
        "{outer_label}"
    );
    assert_eq!(outer_params, ["String", "String"]);
    assert_eq!(outer_active, 1);

    let (ctor_label, ctor_params, ctor_active, _) = signature_label_at(
        &mut compilation,
        &main,
        source,
        r#"mkPacket = Packet 1 "ok""#,
        r#""ok""#,
    );
    assert!(
        ctor_label.contains("Packet : Int -> String -> Packet"),
        "{ctor_label}"
    );
    assert_eq!(ctor_params, ["Int", "String"]);
    assert_eq!(ctor_active, 1);

    let (cons_label, cons_params, cons_active, _) = signature_label_at_last(
        &mut compilation,
        &main,
        source,
        "consed tail = 1 :: tail",
        "tail",
    );
    assert!(
        cons_label.contains("Cons : forall a. a -> [a] -> [a]"),
        "{cons_label}"
    );
    assert_eq!(cons_params.len(), 2);
    assert_eq!(cons_active, 1);

    let (do_label, do_params, do_active, do_docs) = signature_label_at(
        &mut compilation,
        &main,
        source,
        "        printStr helpText",
        "helpText",
    );
    assert!(
        do_label.contains("printStr : String -> IO IOError ()"),
        "{do_label}"
    );
    assert!(!do_label.contains("thenM"));
    assert!(!do_label.contains("bind"));
    assert_eq!(do_params, ["String"]);
    assert_eq!(do_active, 0);
    assert_eq!(
        do_docs.as_deref(),
        Some("Writes text without adding a newline.")
    );

    drop(project_root);
}

#[test]
fn dependency_sources_stay_queryable_but_mutating_features_refuse_them() {
    let project_root = temp_root("dependency_editability");
    let app_root = project_root.join("app");
    let dependency_root = project_root.join("dependency");
    let app = app_root.join("src/main.sv");
    let dependency = dependency_root.join("src/core.sv");
    let app_source = r#"import dependency.core(answer)

main = answer
"#;
    let dependency_source = r#"export (answer)

-- | The dependency answer.
answer = 42
"#;

    write_source(
        &app_root.join("solveig.toml"),
        r#"[package]
name = "app"
version = "0.1.0"

[dependencies]
dependency = { path = "../dependency" }

[language]
entry = "main"
"#,
    );
    write_source(
        &dependency_root.join("solveig.toml"),
        r#"[package]
name = "dependency"
version = "0.1.0"

[language]
entry = "core"
"#,
    );
    write_source(&app, app_source);
    write_source(&dependency, dependency_source);
    let dependency_canonical =
        fs::canonicalize(&dependency).expect("dependency source should canonicalize");

    let mut compilation = Compilation::new(&app_root);
    let result = compilation.check(true).expect("check should finish");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let reference = position_in_line(app_source, "main = answer", "answer");
    let definition = compilation
        .definition_at_position(&app, reference, PositionEncoding::Utf8)
        .expect("dependency definition should remain navigable");
    assert_eq!(
        compilation.source_map().lookup(definition.file).path,
        dependency_canonical
    );

    let references = compilation
        .references_at_position(&app, reference, PositionEncoding::Utf8, true)
        .expect("dependency references should remain available");
    assert!(
        references
            .iter()
            .any(|span| { compilation.source_map().lookup(span.file).path == app })
    );
    assert!(
        references.iter().any(|span| {
            compilation.source_map().lookup(span.file).path == dependency_canonical
        })
    );

    let dependency_position = position_in_line(dependency_source, "answer = 42", "answer");
    let hover = compilation
        .hover_at_position(
            &dependency_canonical,
            dependency_position,
            PositionEncoding::Utf8,
        )
        .expect("dependency hover should remain available");
    assert!(hover.signature().contains("answer : Int"), "{hover:#?}");
    let completions = compilation.completion_items_at_position(
        &dependency_canonical,
        dependency_position,
        PositionEncoding::Utf8,
    );
    assert!(
        completions.iter().any(|item| item.label() == "answer"),
        "dependency completion should remain available: {completions:#?}"
    );

    compilation
        .ensure_source_editable(&app)
        .expect("the local package should remain editable");
    let formatting_error = compilation
        .ensure_source_editable(&dependency_canonical)
        .expect_err("formatting a dependency must be refused");
    assert!(
        formatting_error.to_string().contains("dependency source"),
        "{formatting_error}"
    );
    assert!(
        formatting_error
            .to_string()
            .contains("open that package as the local project"),
        "{formatting_error}"
    );
    let dependency_formatting_error = compilation
        .format_document(&dependency_canonical, 100)
        .expect_err("document formatting must use the dependency edit boundary");
    assert_eq!(dependency_formatting_error, formatting_error);
    let prelude = compilation
        .source_map()
        .files()
        .iter()
        .find(|file| file.path.ends_with("prelude/list.sv"))
        .expect("Prelude list source should be loaded")
        .path
        .clone();
    let implicit_error = compilation
        .ensure_source_editable(&prelude)
        .expect_err("formatting an implicit library must be refused");
    assert!(
        implicit_error
            .to_string()
            .contains("implicit library source"),
        "{implicit_error}"
    );
    let implicit_formatting_error = compilation
        .format_range(
            &prelude,
            SourcePosition::new(0, 0),
            SourcePosition::new(0, 1),
            PositionEncoding::Utf8,
            100,
        )
        .expect_err("range formatting must use the implicit-library edit boundary");
    assert_eq!(implicit_formatting_error, implicit_error);

    let prepare_error = compilation
        .prepare_rename_at_position(&app, reference, PositionEncoding::Utf8)
        .expect_err("renaming a dependency reference must be refused entirely");
    assert_eq!(prepare_error, formatting_error);
    let rename_error = compilation
        .rename_at_position(&app, reference, PositionEncoding::Utf8, "renamedAnswer")
        .expect_err("a dependency-spanning rename must not return partial edits");
    assert_eq!(rename_error, formatting_error);

    let dependency_actions = compilation.code_actions_at_range(
        &dependency_canonical,
        SourcePosition::new(0, 0),
        SourcePosition::new(2, 0),
        PositionEncoding::Utf8,
    );
    assert!(
        dependency_actions.is_empty(),
        "dependency code actions must be filtered: {dependency_actions:#?}"
    );
    let local_actions = compilation.source_code_actions();
    assert!(
        local_actions
            .iter()
            .any(|action| action.title().contains("type signature for `main`")),
        "local code actions should remain available: {local_actions:#?}"
    );
    assert!(
        local_actions
            .iter()
            .flat_map(|action| action.edits())
            .all(|edit| {
                compilation
                    .source_map()
                    .lookup(edit.file())
                    .path
                    .starts_with(&app_root)
            })
    );

    drop(project_root);
}
