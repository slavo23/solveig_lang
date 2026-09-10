#![expect(
    clippy::panic,
    reason = "integration tests use panic messages to explain LSP audit failures"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use solveig_base::{FileId, FileSpan, PositionEncoding, SourceMap, SourcePosition};
use solveig_driver::Compilation;
use solveig_ide::{CompilationIdeExt, SourceInlayHintConfig, SourceSelectionRange, SourceSymbol};
use solveig_syntax::lexer::{Token, lex};

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

#[test]
fn editor_queries_survive_scry_project_token_sweep() {
    assert_editor_queries_survive_token_sweep("examples/scry", 0, 1);
}

#[test]
fn editor_queries_survive_smoke_project_token_sweep_shard_1_of_3() {
    assert_editor_queries_survive_token_sweep("examples/smoke_suite", 0, 3);
}

#[test]
fn editor_queries_survive_smoke_project_token_sweep_shard_2_of_3() {
    assert_editor_queries_survive_token_sweep("examples/smoke_suite", 1, 3);
}

#[test]
fn editor_queries_survive_smoke_project_token_sweep_shard_3_of_3() {
    assert_editor_queries_survive_token_sweep("examples/smoke_suite", 2, 3);
}

fn assert_editor_queries_survive_token_sweep(
    project: &str,
    shard_index: usize,
    shard_count: usize,
) {
    let anomalies = audit_project_shard(&fixture_root(project), shard_index, shard_count);

    assert!(
        anomalies.is_empty(),
        "editor audit shard {}/{} found anomalies in `{project}`:\n{}",
        shard_index + 1,
        shard_count,
        anomalies.join("\n")
    );
}

#[test]
fn case_hover_snapshots_cover_scry_and_smoke_examples() {
    let cases = [
        (
            "examples/scry",
            "src/scry/parser.sv",
            &[
                "    case tokens of",
                "        | _ =>",
                "        | Parsed item rest => parseList rest (item :: items)",
                "                    case parseAllTokens rest of",
                "                        | Just terms => Just (term :: terms)",
            ][..],
            concat!(
                " 13:9   `|` => <none>\n",
                " 13:11  `_` => _ : [Char]\n",
                " 13:13  `=>` => <none>\n",
                " 33:5   `case` => <none>\n",
                " 33:10  `tokens` => tokens : [Tok]\n",
                " 33:17  `of` => <none>\n",
                " 39:17  `|` => <none>\n",
                " 39:19  `Parsed` => Parsed : SExp -> [Tok] -> ParseStep\n",
                " 39:26  `item` => item : SExp\n",
                " 39:31  `rest` => rest : [Tok]\n",
                " 39:36  `=>` => <none>\n",
                " 39:39  `parseList` => parseList : [Tok] -> [SExp] -> ParseStep\n",
                " 39:49  `rest` => rest : [Tok]\n",
                " 39:54  `(` => [SExp]\n",
                " 39:55  `item` => item : SExp\n",
                " 39:60  `::` => Cons : forall a. a -> [a] -> [a]\n",
                " 39:63  `items` => items : [SExp]\n",
                " 39:68  `)` => [SExp]\n",
                " 57:21  `case` => <none>\n",
                " 57:26  `parseAllTokens` => parseAllTokens : [Tok] -> Maybe [SExp]\n",
                " 57:41  `rest` => rest : [Tok]\n",
                " 57:46  `of` => <none>\n",
                " 83:25  `|` => <none>\n",
                " 83:27  `Just` => Just : forall a. a -> Maybe a\n",
                " 83:32  `terms` => terms : [Term]\n",
                " 83:38  `=>` => <none>\n",
                " 83:41  `Just` => Just : forall a. a -> Maybe a\n",
                " 83:46  `(` => [Term]\n",
                " 83:47  `term` => term : Term\n",
                " 83:52  `::` => Cons : forall a. a -> [a] -> [a]\n",
                " 83:55  `terms` => terms : [Term]\n",
                " 83:60  `)` => [Term]\n",
            ),
        ),
        (
            "examples/scry",
            "src/scry/solver.sv",
            &[
                "    in case (walkedLeft, walkedRight) of",
                "        | (term, Var name) => bindVar name term bindings",
                "        | GoalPred pred => solvePredLoop rules rules depth bindings pred",
            ][..],
            concat!(
                " 33:5   `in` => <none>\n",
                " 33:8   `case` => <none>\n",
                " 33:13  `(` => (Term, Term)\n",
                " 33:14  `walkedLeft` => walkedLeft : Term\n",
                " 33:24  `,` => <none>\n",
                " 33:26  `walkedRight` => walkedRight : Term\n",
                " 33:37  `)` => (Term, Term)\n",
                " 33:39  `of` => <none>\n",
                " 36:9   `|` => <none>\n",
                " 36:11  `(` => (Term, Term)\n",
                " 36:12  `term` => term : Term\n",
                " 36:16  `,` => <none>\n",
                " 36:18  `Var` => Var : String -> Term\n",
                " 36:22  `name` => name : String\n",
                " 36:26  `)` => (Term, Term)\n",
                " 36:28  `=>` => <none>\n",
                " 36:31  `bindVar` => bindVar : String -> Term -> [Binding] -> Maybe [Binding]\n",
                " 36:39  `name` => name : String\n",
                " 36:44  `term` => term : Term\n",
                " 36:49  `bindings` => bindings : [Binding]\n",
                "137:9   `|` => <none>\n",
                "137:11  `GoalPred` => GoalPred : Pred -> Goal\n",
                "137:20  `pred` => pred : Pred\n",
                "137:25  `=>` => <none>\n",
                "137:28  `solvePredLoop` => solvePredLoop : [Clause] -> [Clause] -> Int -> [Binding] -> Pred -> [[Binding]]\n",
                "137:42  `rules` => rules : [Clause]\n",
                "137:48  `rules` => rules : [Clause]\n",
                "137:54  `depth` => depth : Int\n",
                "137:60  `bindings` => bindings : [Binding]\n",
                "137:69  `pred` => pred : Pred\n",
            ),
        ),
        (
            "examples/smoke_suite",
            "src/brainfuck.sv",
            &[
                "            case st of",
                "                BFState left cur right input output =>",
                "                    case right of",
                "                        | x :: xs => ((), BFState (cur :: left) x xs input output)",
            ][..],
            concat!(
                " 97:13  `case` => <none>\n",
                " 97:18  `st` => st : BFState\n",
                " 97:21  `of` => <none>\n",
                " 98:17  `BFState` => BFState : [Int] -> Int -> [Int] -> [Int] -> [Int] -> BFState\n",
                " 98:25  `left` => left : [Int]\n",
                " 98:30  `cur` => cur : Int\n",
                " 98:34  `right` => right : [Int]\n",
                " 98:40  `input` => input : [Int]\n",
                " 98:46  `output` => output : [Int]\n",
                " 98:53  `=>` => <none>\n",
                " 99:21  `case` => <none>\n",
                " 99:26  `right` => right : [Int]\n",
                " 99:32  `of` => <none>\n",
                "101:25  `|` => <none>\n",
                "101:27  `x` => x : Int\n",
                "101:29  `::` => Cons : forall a. a -> [a] -> [a]\n",
                "101:32  `xs` => xs : [Int]\n",
                "101:35  `=>` => <none>\n",
                "101:38  `(` => ((), BFState)\n",
                "101:39  `(` => ()\n",
                "101:40  `)` => ()\n",
                "101:41  `,` => <none>\n",
                "101:43  `BFState` => BFState : [Int] -> Int -> [Int] -> [Int] -> [Int] -> BFState\n",
                "101:51  `(` => [Int]\n",
                "101:52  `cur` => cur : Int\n",
                "101:56  `::` => Cons : forall a. a -> [a] -> [a]\n",
                "101:59  `left` => left : [Int]\n",
                "101:63  `)` => [Int]\n",
                "101:65  `x` => x : Int\n",
                "101:67  `xs` => xs : [Int]\n",
                "101:70  `input` => input : [Int]\n",
                "101:76  `output` => output : [Int]\n",
                "101:82  `)` => ((), BFState)\n",
            ),
        ),
    ];

    let mut compilations = BTreeMap::new();
    for project in ["examples/scry", "examples/smoke_suite"] {
        let project_root = fixture_root(project);
        let mut compilation = Compilation::new(&project_root);
        let diagnostics = {
            let result = compilation
                .check(true)
                .unwrap_or_else(|err| panic!("{project} should check: {err}"));
            format!("{:?}", result.diagnostics())
        };
        assert!(
            compilation
                .source_map()
                .files()
                .iter()
                .any(|source| source.path.starts_with(&project_root)),
            "{project} should retain project sources after diagnostics: {diagnostics}"
        );
        compilations.insert(project, compilation);
    }

    for (project, relative_file, lines, expected) in cases {
        let project_root = fixture_root(project);
        let file = project_root.join(relative_file);
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("{} should be readable: {err}", file.display()));
        let compilation = compilations
            .get(project)
            .unwrap_or_else(|| panic!("{project} should have a retained compilation"));

        assert!(
            compilation
                .source_map()
                .files()
                .iter()
                .any(|source| source.path == file),
            "{project} should retain {} for editor queries",
            file.display()
        );

        let actual = hover_audit_for_lines(compilation, &file, &source, lines);
        assert_eq!(
            actual, expected,
            "case-hover snapshot changed for {project}/{relative_file}"
        );
    }
}

fn audit_project_shard(project_root: &Path, shard_index: usize, shard_count: usize) -> Vec<String> {
    let mut compilation = Compilation::new(project_root);
    let result = compilation
        .check(true)
        .unwrap_or_else(|err| panic!("{} should check: {err}", project_root.display()));

    assert!(
        result.success(),
        "{} diagnostics: {:?}",
        project_root.display(),
        result.diagnostics()
    );

    let mut anomalies = Vec::new();
    let project_files = compilation
        .source_map()
        .files()
        .iter()
        .filter(|source| {
            source.path.starts_with(project_root)
                && source
                    .path
                    .extension()
                    .is_some_and(|extension| extension == "sv")
        })
        .map(|source| source.path.clone())
        .collect::<Vec<_>>();
    for file in project_files
        .iter()
        .enumerate()
        .filter_map(|(index, file)| (index % shard_count == shard_index).then_some(file))
    {
        audit_file(&mut compilation, file, &mut anomalies);
    }
    for file in solveig_sources(&project_root.join("src"))
        .into_iter()
        .enumerate()
        .filter_map(|(index, file)| (index % shard_count == shard_index).then_some(file))
    {
        if project_files.contains(&file) {
            continue;
        }
        audit_standalone_file(&file, &mut anomalies);
    }

    anomalies
}

fn hover_audit_for_lines(
    compilation: &Compilation,
    file: &Path,
    source: &str,
    lines: &[&str],
) -> String {
    let tokens = lex(FileId(0), source).expect("hover audit source should lex");
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

    for token in tokens
        .iter()
        .filter(|token| !token.is_synthetic() && !token.kind.is_trivia())
    {
        if !line_ranges
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

fn audit_standalone_file(file: &Path, anomalies: &mut Vec<String>) {
    let mut compilation = Compilation::for_source_file(file);
    let result = compilation.check(true).unwrap_or_else(|err| {
        panic!(
            "{} should check as a standalone source: {err}",
            file.display()
        )
    });

    if result.success() && source_file_id(compilation.source_map(), file).is_some() {
        audit_file(&mut compilation, file, anomalies);
    }
}

fn audit_file(compilation: &mut Compilation, file: &Path, anomalies: &mut Vec<String>) {
    let source = fs::read_to_string(file)
        .unwrap_or_else(|err| panic!("{} should be readable: {err}", file.display()));
    let file_id = source_file_id(compilation.source_map(), file)
        .unwrap_or_else(|| panic!("source map should include {}", file.display()));

    audit_file_level_features(compilation, file, file_id, source.len(), anomalies);

    let tokens = lex(FileId(0), &source)
        .unwrap_or_else(|err| panic!("{} should lex: {err:?}", file.display()));

    for token in tokens
        .iter()
        .filter(|token| !token.is_synthetic() && !token.kind.is_trivia())
    {
        let text = token.source_text(&source);
        if text.is_empty() {
            continue;
        }

        let position = compilation.source_map().position(
            file_id,
            test_u32(token.start(), "token byte offset"),
            PositionEncoding::Utf8,
        );
        audit_token_features(compilation, file, position, token, text, anomalies);
    }

    for position in line_end_positions(&source) {
        let _ = compilation.completion_items_at_position(file, position, PositionEncoding::Utf8);
    }
}

fn audit_file_level_features(
    compilation: &mut Compilation,
    file: &Path,
    file_id: solveig_base::FileId,
    source_len: usize,
    anomalies: &mut Vec<String>,
) {
    for symbol in compilation.document_symbols(file) {
        audit_symbol(compilation.source_map(), &symbol, anomalies);
    }

    let semantic_tokens = compilation.semantic_tokens(file);
    let mut previous: Option<FileSpan> = None;
    for token in semantic_tokens {
        let span = token.file_span();
        validate_span(
            compilation.source_map(),
            span,
            false,
            "semantic token",
            anomalies,
        );
        if let Some(previous) = previous
            && previous.file == span.file
            && previous.span.end > span.span.start
        {
            anomalies.push(format!(
                "semantic tokens overlap: {previous:?} then {span:?}"
            ));
        }
        previous = Some(span);
    }

    for range in compilation.folding_ranges(file) {
        validate_span(
            compilation.source_map(),
            range.file_span(),
            false,
            "folding range",
            anomalies,
        );
    }

    let start = SourcePosition::new(0, 0);
    let end = compilation.source_map().position(
        file_id,
        test_u32(source_len, "source length"),
        PositionEncoding::Utf8,
    );
    for hint in compilation.inlay_hints_at_range(
        file,
        start,
        end,
        PositionEncoding::Utf8,
        SourceInlayHintConfig::default(),
    ) {
        validate_span(
            compilation.source_map(),
            hint.file_span(),
            true,
            "inlay hint",
            anomalies,
        );
        if hint.label().trim().is_empty() {
            anomalies.push(format!("empty inlay hint label at {:?}", hint.file_span()));
        }
    }

    for link in compilation.document_links(file) {
        validate_span(
            compilation.source_map(),
            link.file_span(),
            false,
            "document link",
            anomalies,
        );
        if link.target().as_os_str().is_empty() {
            anomalies.push(format!(
                "empty document-link target at {:?}",
                link.file_span()
            ));
        }
    }
}

fn audit_token_features(
    compilation: &mut Compilation,
    file: &Path,
    position: SourcePosition,
    token: &solveig_syntax::lexer::PosToken,
    text: &str,
    anomalies: &mut Vec<String>,
) {
    if let Some(info) = compilation.hover_info(file, token.start()) {
        validate_span(
            compilation.source_map(),
            info.file_span(),
            false,
            "hover",
            anomalies,
        );
        if syntax_token_should_not_hover(&token.kind) {
            anomalies.push(format!(
                "{}:{} syntax token `{text}` unexpectedly hovers as `{}`",
                token.line + 1,
                token.col + 1,
                info.signature()
            ));
        }
        if contains_generated_hover_helper(text, info.signature()) {
            anomalies.push(format!(
                "{}:{} hover leaks generated helper: `{}`",
                token.line + 1,
                token.col + 1,
                info.signature()
            ));
        }
    }

    if let Some(definition) = compilation.definition(file, token.start()) {
        validate_span(
            compilation.source_map(),
            definition,
            false,
            "definition",
            anomalies,
        );
        if span_text(compilation.source_map(), definition)
            .is_some_and(contains_generated_span_helper)
        {
            anomalies.push(format!(
                "{}:{} definition points at generated helper",
                token.line + 1,
                token.col + 1
            ));
        }
    }

    if let Some(references) = compilation.references(file, token.start(), false) {
        for reference in references {
            validate_span(
                compilation.source_map(),
                reference,
                false,
                "reference",
                anomalies,
            );
        }
    }

    for highlight in
        compilation.document_highlights_at_position(file, position, PositionEncoding::Utf8)
    {
        validate_span(
            compilation.source_map(),
            highlight.file_span(),
            false,
            "document highlight",
            anomalies,
        );
    }

    for range in compilation.selection_ranges(file, &[position], PositionEncoding::Utf8) {
        audit_selection_range(compilation.source_map(), &range, anomalies);
    }

    if let Some(help) =
        compilation.signature_help_at_position(file, position, PositionEncoding::Utf8)
        && help.signatures().is_empty()
    {
        anomalies.push(format!(
            "{}:{} signature help has no signatures",
            token.line + 1,
            token.col + 1
        ));
    }
}

fn solveig_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_solveig_sources(root, &mut out);
    out.sort();
    out
}

fn collect_solveig_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("{} should be readable: {err}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|err| panic!("{} entry should be readable: {err}", dir.display()))
            .path();
        if path.is_dir() {
            collect_solveig_sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "sv") {
            out.push(path);
        }
    }
}

fn source_file_id(source_map: &SourceMap, file: &Path) -> Option<solveig_base::FileId> {
    source_map
        .files()
        .iter()
        .enumerate()
        .find_map(|(index, source)| {
            (source.path == file || source.path.ends_with(file) || file.ends_with(&source.path))
                .then(|| solveig_base::FileId(test_u32(index, "source file index")))
        })
}

fn line_end_positions(source: &str) -> Vec<SourcePosition> {
    source
        .lines()
        .enumerate()
        .map(|(line, text)| {
            SourcePosition::new(
                test_u32(line, "line index"),
                test_u32(text.len(), "line byte length"),
            )
        })
        .collect()
}

fn test_u32(value: usize, what: &str) -> u32 {
    u32::try_from(value).unwrap_or_else(|_| panic!("{what} should fit in u32"))
}

fn audit_symbol(source_map: &SourceMap, symbol: &SourceSymbol, anomalies: &mut Vec<String>) {
    validate_span(
        source_map,
        symbol.file_span(),
        false,
        "document symbol",
        anomalies,
    );
    validate_span(
        source_map,
        symbol.selection_span(),
        false,
        "document symbol selection",
        anomalies,
    );
    if symbol.name().trim().is_empty() {
        anomalies.push(format!(
            "empty document symbol name at {:?}",
            symbol.file_span()
        ));
    }
    for child in symbol.children() {
        audit_symbol(source_map, child, anomalies);
    }
}

fn audit_selection_range(
    source_map: &SourceMap,
    range: &SourceSelectionRange,
    anomalies: &mut Vec<String>,
) {
    validate_span(
        source_map,
        range.file_span(),
        false,
        "selection range",
        anomalies,
    );
    if let Some(parent) = range.parent() {
        validate_span(
            source_map,
            parent.file_span(),
            false,
            "selection range parent",
            anomalies,
        );
    }
}

fn validate_span(
    source_map: &SourceMap,
    span: FileSpan,
    allow_zero: bool,
    context: &str,
    anomalies: &mut Vec<String>,
) {
    let Some(source) = source_map.files().get(span.file.0 as usize) else {
        anomalies.push(format!(
            "{context} span uses missing file id {:?}",
            span.file
        ));
        return;
    };
    let range = span.span.to_usize_range();
    if range.start > range.end || range.end > source.src.len() {
        anomalies.push(format!(
            "{context} span {:?} is outside {} (len {})",
            span,
            source.path.display(),
            source.src.len()
        ));
    }
    if !allow_zero && range.start == range.end {
        anomalies.push(format!("{context} span {span:?} is empty"));
    }
}

fn span_text(source_map: &SourceMap, span: FileSpan) -> Option<&str> {
    let source = source_map.files().get(span.file.0 as usize)?;
    source.src.get(span.span.to_usize_range())
}

fn contains_generated_hover_helper(token_text: &str, text: &str) -> bool {
    text.contains("thenM") || (token_text != "bind" && text.contains("bind :"))
}

fn contains_generated_span_helper(text: &str) -> bool {
    text.contains("thenM") || text.contains("bind :")
}

fn syntax_token_should_not_hover(kind: &Token) -> bool {
    matches!(
        kind,
        Token::Whitespace
            | Token::Newline
            | Token::LineComment
            | Token::BlockComment
            | Token::Let
            | Token::In
            | Token::Do
            | Token::Fun
            | Token::Forall
            | Token::Case
            | Token::Of
            | Token::If
            | Token::Then
            | Token::Else
            | Token::Data
            | Token::Type
            | Token::Sig
            | Token::Class
            | Token::Instance
            | Token::Where
            | Token::Default
            | Token::Import
            | Token::As
            | Token::Infixl
            | Token::Infixr
            | Token::Infix
            | Token::Eq
            | Token::Colon
            | Token::Arrow
            | Token::BindArrow
            | Token::FatArrow
            | Token::Pipe
            | Token::Dot
            | Token::Comma
            | Token::Semicolon
            | Token::Backtick
            | Token::VOpen
            | Token::VClose
            | Token::VSemi
            | Token::Eof
    )
}
