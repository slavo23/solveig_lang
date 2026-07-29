use std::fs;
use std::path::{Path, PathBuf};

use solveig_base::{FileSpan, PositionEncoding, Session, SourceMap, SourcePosition};
use solveig_driver::{Compilation, SourceInlayHintConfig, SourceSelectionRange, SourceSymbol};
use solveig_syntax::lexer::{Token, lex};

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

#[test]
fn editor_queries_survive_scry_and_smoke_suite_token_sweeps() {
    for project in ["examples/scry", "examples/smoke_suite"] {
        let anomalies = audit_project(&fixture_root(project));

        assert!(
            anomalies.is_empty(),
            "editor audit found anomalies in `{project}`:\n{}",
            anomalies.join("\n")
        );
    }
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
                " 16:9   `|` => <none>\n",
                " 16:11  `_` => _ : List Char\n",
                " 16:13  `=>` => <none>\n",
                " 37:5   `case` => <none>\n",
                " 37:10  `tokens` => tokens : List Tok\n",
                " 37:17  `of` => <none>\n",
                " 43:17  `|` => <none>\n",
                " 43:19  `Parsed` => Parsed : SExp -> List Tok -> ParseStep\n",
                " 43:26  `item` => item : SExp\n",
                " 43:31  `rest` => rest : List Tok\n",
                " 43:36  `=>` => <none>\n",
                " 43:39  `parseList` => parseList : List Tok -> List SExp -> ParseStep\n",
                " 43:49  `rest` => rest : List Tok\n",
                " 43:54  `(` => List SExp\n",
                " 43:55  `item` => item : SExp\n",
                " 43:60  `::` => Cons : ∀ (a : *). a -> List a -> List a\n",
                " 43:63  `items` => items : List SExp\n",
                " 43:68  `)` => List SExp\n",
                " 61:21  `case` => <none>\n",
                " 61:26  `parseAllTokens` => parseAllTokens : List Tok -> Maybe (List SExp)\n",
                " 61:41  `rest` => rest : List Tok\n",
                " 61:46  `of` => <none>\n",
                " 91:25  `|` => <none>\n",
                " 91:27  `Just` => Just : ∀ (a : *). a -> Maybe a\n",
                " 91:32  `terms` => terms : List Term\n",
                " 91:38  `=>` => <none>\n",
                " 91:41  `Just` => Just : ∀ (a : *). a -> Maybe a\n",
                " 91:46  `(` => List Term\n",
                " 91:47  `term` => term : Term\n",
                " 91:52  `::` => Cons : ∀ (a : *). a -> List a -> List a\n",
                " 91:55  `terms` => terms : List Term\n",
                " 91:60  `)` => List Term\n",
            ),
        ),
        (
            "examples/scry",
            "src/scry/solver.sv",
            &[
                "        case MkPair walkedLeft walkedRight of",
                "            | MkPair term (Var name) => bindVar name term bindings",
                "        | GoalPred pred => solvePredLoop rules rules depth bindings pred",
            ][..],
            concat!(
                " 36:9   `case` => <none>\n",
                " 36:14  `MkPair` => MkPair : ∀ (a : *) (b : *). a -> b -> Pair a b\n",
                " 36:21  `walkedLeft` => walkedLeft : Term\n",
                " 36:32  `walkedRight` => walkedRight : Term\n",
                " 36:44  `of` => <none>\n",
                " 40:13  `|` => <none>\n",
                " 40:15  `MkPair` => MkPair : ∀ (a : *) (b : *). a -> b -> Pair a b\n",
                " 40:22  `term` => term : Term\n",
                " 40:27  `(` => <none>\n",
                " 40:28  `Var` => Var : String -> Term\n",
                " 40:32  `name` => name : String\n",
                " 40:36  `)` => <none>\n",
                " 40:38  `=>` => <none>\n",
                " 40:41  `bindVar` => bindVar : String -> Term -> List Binding -> Maybe (List Binding)\n",
                " 40:49  `name` => name : String\n",
                " 40:54  `term` => term : Term\n",
                " 40:59  `bindings` => bindings : List Binding\n",
                "150:9   `|` => <none>\n",
                "150:11  `GoalPred` => GoalPred : Pred -> Goal\n",
                "150:20  `pred` => pred : Pred\n",
                "150:25  `=>` => <none>\n",
                "150:28  `solvePredLoop` => solvePredLoop : List Clause -> List Clause -> Int -> List Binding -> Pred -> List (List Binding)\n",
                "150:42  `rules` => rules : List Clause\n",
                "150:48  `rules` => rules : List Clause\n",
                "150:54  `depth` => depth : Int\n",
                "150:60  `bindings` => bindings : List Binding\n",
                "150:69  `pred` => pred : Pred\n",
            ),
        ),
        (
            "examples/smoke_suite",
            "src/brainfuck.sv",
            &[
                "            case st of",
                "                BFState left cur right input output =>",
                "                    case right of",
                "                      | x :: xs =>",
                "                            Pair () (BFState (cur :: left) x xs input output));",
            ][..],
            concat!(
                " 89:13  `case` => <none>\n",
                " 89:18  `st` => st : BFState\n",
                " 89:21  `of` => <none>\n",
                " 90:17  `BFState` => BFState : List Int -> Int -> List Int -> List Int -> List Int -> BFState\n",
                " 90:25  `left` => left : List Int\n",
                " 90:30  `cur` => cur : Int\n",
                " 90:34  `right` => right : List Int\n",
                " 90:40  `input` => input : List Int\n",
                " 90:46  `output` => output : List Int\n",
                " 90:53  `=>` => <none>\n",
                " 91:21  `case` => <none>\n",
                " 91:26  `right` => right : List Int\n",
                " 91:32  `of` => <none>\n",
                " 94:23  `|` => <none>\n",
                " 94:25  `x` => x : Int\n",
                " 94:27  `::` => Cons : ∀ (a : *). a -> List a -> List a\n",
                " 94:30  `xs` => xs : List Int\n",
                " 94:33  `=>` => <none>\n",
                " 95:29  `Pair` => Pair : ∀ (a : *) (b : *). a -> b -> Pair a b\n",
                " 95:34  `(` => ()\n",
                " 95:35  `)` => ()\n",
                " 95:37  `(` => BFState\n",
                " 95:38  `BFState` => BFState : List Int -> Int -> List Int -> List Int -> List Int -> BFState\n",
                " 95:46  `(` => List Int\n",
                " 95:47  `cur` => cur : Int\n",
                " 95:51  `::` => Cons : ∀ (a : *). a -> List a -> List a\n",
                " 95:54  `left` => left : List Int\n",
                " 95:58  `)` => List Int\n",
                " 95:60  `x` => x : Int\n",
                " 95:62  `xs` => xs : List Int\n",
                " 95:65  `input` => input : List Int\n",
                " 95:71  `output` => output : List Int\n",
                " 95:77  `)` => BFState\n",
                " 95:78  `)` => BFState -> Pair () BFState\n",
                " 95:79  `;` => <none>\n",
            ),
        ),
    ];

    for (project, relative_file, lines, expected) in cases {
        let project_root = fixture_root(project);
        let file = project_root.join(relative_file);
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("{} should be readable: {err}", file.display()));
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
                .any(|source| source.path == file),
            "{project} should retain {} for editor queries after diagnostics: {}",
            file.display(),
            diagnostics
        );

        let actual = hover_audit_for_lines(&compilation, &file, &source, lines);
        assert_eq!(
            actual, expected,
            "case-hover snapshot changed for {project}/{relative_file}"
        );
    }
}

fn audit_project(project_root: &Path) -> Vec<String> {
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
    for file in &project_files {
        audit_file(&mut compilation, file, &mut anomalies);
    }
    for file in solveig_sources(&project_root.join("src")) {
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
    let lex_session = Session::new();
    let lex_file = lex_session.add_file(file, source.to_string());
    let tokens = lex(&lex_session, lex_file, source).expect("hover audit source should lex");
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
            .any(|(start, end)| *start <= token.start && token.end <= *end)
        {
            continue;
        }

        let text = token.source_text(source);
        if text.is_empty() {
            continue;
        }

        let signature = compilation.hover_info(file, token.start).map_or_else(
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

    let lex_session = Session::new();
    let lex_file = lex_session.add_file(file, source.clone());
    let tokens = lex(&lex_session, lex_file, &source)
        .unwrap_or_else(|err| panic!("{} should lex: {err:?}", file.display()));

    for token in tokens
        .iter()
        .filter(|token| !token.is_synthetic() && !token.kind.is_trivia())
    {
        let text = token.source_text(&source);
        if text.is_empty() {
            continue;
        }

        let position =
            compilation
                .source_map()
                .position(file_id, token.start as u32, PositionEncoding::Utf8);
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
    let end = compilation
        .source_map()
        .position(file_id, source_len as u32, PositionEncoding::Utf8);
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
    if let Some(info) = compilation.hover_info(file, token.start) {
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

    if let Some(definition) = compilation.definition(file, token.start) {
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

    if let Some(references) = compilation.references(file, token.start, false) {
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
                .then_some(solveig_base::FileId(index as u32))
        })
}

fn line_end_positions(source: &str) -> Vec<SourcePosition> {
    source
        .lines()
        .enumerate()
        .map(|(line, text)| SourcePosition::new(line as u32, text.len() as u32))
        .collect()
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
