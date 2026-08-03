use solveig_base::{FileSpan, PositionEncoding, SourceMap, SourcePosition};
use solveig_driver::Compilation;
use solveig_ide::{CompilationIdeExt, SourceDocumentHighlightKind, SourceFoldingRangeKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "solveig_editor_protocol_{name}_{}_{}",
        process::id(),
        nanos
    ))
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
    let line_number = source[..line_start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count();

    SourcePosition::new(line_number as u32, needle_offset as u32)
}

fn source_text(source_map: &SourceMap, span: FileSpan) -> &str {
    source_map
        .lookup(span.file)
        .src
        .get(span.span.to_usize_range())
        .expect("span should slice retained source")
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

    let _ = fs::remove_dir_all(project_root);
}
