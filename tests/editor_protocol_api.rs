#![expect(
    clippy::panic,
    reason = "integration tests use panic messages to explain fixture lookup failures"
)]

use solveig_base::{FileSpan, PositionEncoding, SourceMap, SourcePosition};
use solveig_driver::Compilation;
use solveig_ide::{CompilationIdeExt, SourceDocumentHighlightKind, SourceFoldingRangeKind};
use std::fs;
use std::path::Path;

mod support;

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
        .expect("increment should be renameable");
    assert_eq!(
        source_text(compilation.source_map(), rename_span),
        "increment"
    );
    let edits =
        compilation.rename_at_position(&main, increment_position, PositionEncoding::Utf8, "bump");
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

main : IO ()
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
        do_label.contains("printStr : String -> IO ()"),
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
