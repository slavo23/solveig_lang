use std::fs;
use std::path::{Path, PathBuf};

use solveig_base::Session;
use solveig_driver::Compilation;
use solveig_syntax::lexer::lex;

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
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

    for token in tokens {
        if token.is_synthetic()
            || token.kind.is_trivia()
            || !line_ranges
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
        Some(&"∀ (element : *). element -> element".to_string())
    );
    assert_eq!(
        compilation.hover(&file, value_offset),
        Some("value : element".to_string())
    );
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
    assert!(
        imported_type
            .provenance()
            .is_some_and(|provenance| provenance.contains("hoverLib")),
        "imported type hover should include provenance: {imported_type:#?}"
    );

    let operator = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    inner + 1", "+"),
        )
        .expect("operator hover should exist");
    assert_contains(operator.signature(), "class Num a");
    assert_contains(operator.signature(), "+ :");

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

    let do_expression = compilation
        .hover_info(
            &main_file,
            offset_in_line(&main_source, "    pure (x + y)", "pure"),
        )
        .expect("do-block expression hover should exist");
    assert_contains(do_expression.signature(), "Applicative f");
    assert_contains(do_expression.signature(), "pure :");
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

    let lambda_line = "statePure value = State (fun state => MkPair value state)";
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
                "        MkPair question answer => QuizCard question answer",
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
                "        MkPair question answer => QuizCard question answer",
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
    assert_eq!(ask_cards.signature(), "cards : List QuizCard");
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
            "statePure value = State (fun state => MkPair value state)",
            "askCards cards = mapM askCard cards",
            "        MkPair question answer => QuizCard question answer",
            "        responses <- askCards cards",
        ],
    );
    assert!(!actual.contains("thenM"));
    assert!(!actual.contains("bind :"));

    let expected = concat!(
        " 13:1   `statePure` => statePure : ∀ (s : *) (a : *). a -> State s a\n",
        " 13:11  `value` => value : a\n",
        " 13:17  `=` => <none>\n",
        " 13:19  `State` => State : ∀ (s : *) (a : *). (s -> Pair a s) -> State s a\n",
        " 13:25  `(` => <none>\n",
        " 13:26  `fun` => <none>\n",
        " 13:30  `state` => state : s\n",
        " 13:36  `=>` => <none>\n",
        " 13:39  `MkPair` => MkPair : ∀ (a : *) (b : *). a -> b -> Pair a b\n",
        " 13:46  `value` => value : a\n",
        " 13:52  `state` => state : s\n",
        " 13:57  `)` => <none>\n",
        " 74:9   `MkPair` => MkPair : ∀ (a : *) (b : *). a -> b -> Pair a b\n",
        " 74:16  `question` => question : String\n",
        " 74:25  `answer` => answer : String\n",
        " 74:32  `=>` => <none>\n",
        " 74:35  `QuizCard` => QuizCard : String -> String -> QuizCard\n",
        " 74:44  `question` => question : String\n",
        " 74:53  `answer` => answer : String\n",
        " 90:1   `askCards` => askCards : List QuizCard -> IO (List (Pair QuizCard String))\n",
        " 90:10  `cards` => cards : List QuizCard\n",
        " 90:16  `=` => <none>\n",
        " 90:18  `mapM` => mapM : ∀ (m : * -> *) (a : *) (b : *). Monad m => (a -> m b) -> List a -> m (List b)\n",
        " 90:23  `askCard` => askCard : QuizCard -> IO (Pair QuizCard String)\n",
        " 90:31  `cards` => cards : List QuizCard\n",
        "136:9   `responses` => responses : List (Pair QuizCard String)\n",
        "136:19  `<-` => <none>\n",
        "136:22  `askCards` => askCards : List QuizCard -> IO (List (Pair QuizCard String))\n",
        "136:31  `cards` => cards : List QuizCard\n",
    );

    assert_eq!(actual, expected);
}
