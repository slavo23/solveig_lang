#![expect(
    clippy::panic,
    reason = "integration tests use panic messages to explain navigation fixture failures"
)]

use std::fs;
use std::path::{Path, PathBuf};

use solveig_base::{FileSpan, SourceMap};
use solveig_driver::Compilation;
use solveig_ide::{CompilationIdeExt, SourceSymbol, SourceSymbolKind};

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

fn offset_in_line(source: &str, line: &str, needle: &str) -> usize {
    let line_start = source
        .find(line)
        .unwrap_or_else(|| panic!("navigation fixture should contain line `{line}`"));
    let needle_offset = line
        .find(needle)
        .unwrap_or_else(|| panic!("navigation fixture line should contain `{needle}`"));

    line_start + needle_offset
}

fn source_for_span<'a>(
    source_map: &SourceMap,
    span: FileSpan,
    main: &'a str,
    lib: &'a str,
) -> &'a str {
    let file = source_map
        .files()
        .get(span.file.0 as usize)
        .expect("navigation span should reference a retained source file");
    let source = if file.path.ends_with("main.sv") {
        main
    } else if file.path.ends_with("hoverLib.sv") {
        lib
    } else {
        panic!("unexpected navigation span file {}", file.path.display());
    };

    source
        .get(span.span.to_usize_range())
        .expect("navigation span should be on UTF-8 boundaries")
}

fn assert_definition_text(
    compilation: &Compilation,
    file: &Path,
    source: &str,
    line: &str,
    needle: &str,
    main_source: &str,
    lib_source: &str,
) {
    assert_definition_text_at_offset(
        compilation,
        file,
        offset_in_line(source, line, needle),
        needle,
        main_source,
        lib_source,
    );
}

fn assert_definition_text_at_offset(
    compilation: &Compilation,
    file: &Path,
    offset: usize,
    expected: &str,
    main_source: &str,
    lib_source: &str,
) {
    let span = compilation
        .definition(file, offset)
        .unwrap_or_else(|| panic!("definition for `{expected}` should exist"));

    assert_eq!(
        source_for_span(compilation.source_map(), span, main_source, lib_source),
        expected
    );
}

fn symbol_named<'a>(symbols: &'a [SourceSymbol], name: &str) -> &'a SourceSymbol {
    symbols
        .iter()
        .find(|symbol| symbol.name() == name)
        .unwrap_or_else(|| panic!("document symbols should include `{name}`"))
}

#[test]
fn definition_covers_imported_semantic_targets() {
    let project_root = fixture_root("tests/fixtures/hover");
    let main_file = project_root.join("src/main.sv");
    let lib_file = project_root.join("src/hoverLib.sv");
    let main_source =
        fs::read_to_string(&main_file).expect("navigation fixture source should load");
    let lib_source = fs::read_to_string(&lib_file).expect("navigation lib fixture should load");
    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("navigation fixture should check");

    assert!(
        result.success(),
        "navigation fixture diagnostics: {:?}",
        result.diagnostics()
    );
    assert_definition_text(
        &compilation,
        &main_file,
        &main_source,
        "import hoverLib(documentedIncrement, Documented, MkDocumented, Describe)",
        "documentedIncrement",
        &main_source,
        &lib_source,
    );
    assert_definition_text(
        &compilation,
        &main_file,
        &main_source,
        "import hoverLib(documentedIncrement, Documented, MkDocumented, Describe)",
        "Describe",
        &main_source,
        &lib_source,
    );
    assert_definition_text(
        &compilation,
        &main_file,
        &main_source,
        "topLevel n = documentedIncrement n",
        "documentedIncrement",
        &main_source,
        &lib_source,
    );
    assert_definition_text(
        &compilation,
        &main_file,
        &main_source,
        "useImported : Documented -> Documented",
        "Documented",
        &main_source,
        &lib_source,
    );
    assert_definition_text(
        &compilation,
        &main_file,
        &main_source,
        "        MkDocumented n => MkDocumented (documentedIncrement n)",
        "MkDocumented",
        &main_source,
        &lib_source,
    );
    let class_offset = offset_in_line(
        &main_source,
        "useDescriber : forall a. Describe a => a -> String",
        ". ",
    ) + ". ".len();
    assert_definition_text_at_offset(
        &compilation,
        &main_file,
        class_offset,
        "Describe",
        &main_source,
        &lib_source,
    );
    assert_definition_text(
        &compilation,
        &main_file,
        &main_source,
        "useDescriber value = describe value",
        "describe",
        &main_source,
        &lib_source,
    );
}

#[test]
fn references_are_project_wide_and_respect_declaration_toggle() {
    let project_root = fixture_root("tests/fixtures/hover");
    let main_file = project_root.join("src/main.sv");
    let main_source =
        fs::read_to_string(&main_file).expect("navigation fixture source should load");
    let lib_source = fs::read_to_string(project_root.join("src/hoverLib.sv"))
        .expect("navigation lib fixture should load");
    let mut compilation = Compilation::new(&project_root);
    let result = compilation
        .check(true)
        .expect("navigation fixture should check");

    assert!(
        result.success(),
        "navigation fixture diagnostics: {:?}",
        result.diagnostics()
    );

    let offset = offset_in_line(
        &main_source,
        "        MkDocumented n => MkDocumented (documentedIncrement n)",
        "documentedIncrement",
    );
    let without_declaration = compilation
        .references(&main_file, offset, false)
        .expect("references without declarations should exist");
    let with_declaration = compilation
        .references(&main_file, offset, true)
        .expect("references with declarations should exist");

    assert_eq!(without_declaration.len(), 3);
    assert!(with_declaration.len() > without_declaration.len());
    assert!(without_declaration.iter().any(|span| {
        source_for_span(compilation.source_map(), *span, &main_source, &lib_source)
            == "documentedIncrement"
            && span.file == source_lookup_main_file_id(compilation.source_map(), &main_file)
    }));
    assert!(without_declaration.iter().all(|span| source_for_span(
        compilation.source_map(),
        *span,
        &main_source,
        &lib_source
    ) == "documentedIncrement"));
    assert!(with_declaration.iter().any(|span| {
        source_for_span(compilation.source_map(), *span, &main_source, &lib_source)
            == "documentedIncrement"
            && span.file
                != without_declaration
                    .first()
                    .expect("reference list should not be empty")
                    .file
    }));
}

fn source_lookup_main_file_id(source_map: &SourceMap, file: &Path) -> solveig_base::FileId {
    source_map
        .files()
        .iter()
        .enumerate()
        .find_map(|(index, source)| {
            (source.path == file)
                .then(|| solveig_base::FileId(test_u32(index, "source file index")))
        })
        .expect("source map should contain main file")
}

fn test_u32(value: usize, what: &str) -> u32 {
    u32::try_from(value).unwrap_or_else(|_| panic!("{what} should fit in u32"))
}

#[test]
fn symbols_are_hierarchical_and_searchable() {
    let project_root = fixture_root("tests/fixtures/hover");
    let lib_file = project_root.join("src/hoverLib.sv");
    let mut compilation = Compilation::new(&project_root);

    let lib_symbols = compilation.document_symbols(&lib_file);
    let documented = symbol_named(&lib_symbols, "Documented");
    let describe_class = symbol_named(&lib_symbols, "Describe");

    assert_eq!(
        symbol_named(&lib_symbols, "documentedIncrement").kind(),
        SourceSymbolKind::Function
    );
    assert_eq!(documented.kind(), SourceSymbolKind::Type);
    assert_eq!(
        symbol_named(documented.children(), "MkDocumented").kind(),
        SourceSymbolKind::Constructor
    );
    assert_eq!(describe_class.kind(), SourceSymbolKind::Class);
    assert_eq!(
        symbol_named(describe_class.children(), "describe").kind(),
        SourceSymbolKind::Method
    );

    let describe_symbols = compilation.workspace_symbols("describe");
    let names = describe_symbols
        .iter()
        .map(|symbol| symbol.name())
        .collect::<Vec<_>>();

    assert!(names.contains(&"Describe"));
    assert!(names.contains(&"describe"));
}
