use solveig_base::{PositionEncoding, SourcePosition};
use solveig_driver::Compilation;
use solveig_ide::{
    CompilationIdeExt, SourceInlayHint, SourceInlayHintCategory, SourceInlayHintConfig,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let thread_id = format!("{:?}", std::thread::current().id());
    std::env::temp_dir().join(format!(
        "solveig_inlay_hints_{name}_{}_{}_{}",
        process::id(),
        thread_id,
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

fn end_position(source: &str) -> SourcePosition {
    let line = source.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let line_start = source.rfind('\n').map_or(0, |index| index + 1);
    SourcePosition::new(line, (source.len() - line_start) as u32)
}

fn inlay_hints(source: &str, config: SourceInlayHintConfig) -> Vec<SourceInlayHint> {
    let root = temp_root("api");
    let source_path = root.join("scratch.sv");

    write_source(&source_path, source);
    let mut compilation = Compilation::for_source_file(&source_path);
    let hints = compilation.inlay_hints_at_range(
        &source_path,
        SourcePosition::new(0, 0),
        end_position(source),
        PositionEncoding::Utf8,
        config,
    );

    let _ = fs::remove_dir_all(root);
    hints
}

fn assert_hint(hints: &[SourceInlayHint], category: SourceInlayHintCategory, label: &str) {
    hints
        .iter()
        .find(|hint| hint.category() == category && hint.label() == label)
        .unwrap_or_else(|| panic!("missing {category:?} hint `{label}` in {hints:#?}"));
}

#[test]
fn inlay_hints_include_let_lambda_function_params_and_inferred_signatures() {
    let hints = inlay_hints(
        r#"useArg value =
    if value then 1 else 0

useLet =
    let local = 41 in
    local

useLambda =
    (fun value => value) 41
"#,
        SourceInlayHintConfig::default(),
    );

    assert_hint(&hints, SourceInlayHintCategory::LetBindingType, ": Int");
    assert_hint(
        &hints,
        SourceInlayHintCategory::FunctionParameterType,
        ": Bool",
    );
    assert_hint(
        &hints,
        SourceInlayHintCategory::LambdaParameterType,
        ": Int",
    );
    assert_hint(
        &hints,
        SourceInlayHintCategory::InferredSignature,
        ": Bool -> Int",
    );
    let signature_hints = hints
        .iter()
        .filter(|hint| hint.category() == SourceInlayHintCategory::InferredSignature)
        .collect::<Vec<_>>();
    assert_eq!(signature_hints.len(), 3, "{hints:#?}");
    assert!(
        signature_hints
            .iter()
            .all(|hint| !hint.label().contains("->") || hint.label() == ": Bool -> Int"),
        "{hints:#?}"
    );
}

#[test]
fn inferred_signature_inlay_uses_source_friendly_scheme_text() {
    let hints = inlay_hints(
        r#"double x = x + x
"#,
        SourceInlayHintConfig::default(),
    );

    assert_hint(
        &hints,
        SourceInlayHintCategory::InferredSignature,
        ": forall a. Num a => a -> a",
    );
}

#[test]
fn inlay_hint_config_disables_categories() {
    let hints = inlay_hints(
        r#"main =
    let local = 41 in
    (fun value => value) local
"#,
        SourceInlayHintConfig {
            let_binding_types: true,
            function_parameter_types: false,
            lambda_parameter_types: false,
            inferred_signatures: false,
        },
    );

    assert_hint(&hints, SourceInlayHintCategory::LetBindingType, ": Int");
    assert!(
        hints
            .iter()
            .all(|hint| hint.category() == SourceInlayHintCategory::LetBindingType),
        "{hints:#?}"
    );
}

#[test]
fn inlay_hints_include_do_let_bindings() {
    let hints = inlay_hints(
        r#"main : Maybe Int
main = do
    let local = 41
    pure local
"#,
        SourceInlayHintConfig::default(),
    );

    assert_hint(&hints, SourceInlayHintCategory::LetBindingType, ": Int");
}

#[test]
fn inlay_hints_skip_annotated_lambda_parameters() {
    let hints = inlay_hints(
        r#"main =
    (fun (value : Int) => value) 41
"#,
        SourceInlayHintConfig::default(),
    );

    assert!(
        hints
            .iter()
            .all(|hint| hint.category() != SourceInlayHintCategory::LambdaParameterType),
        "{hints:#?}"
    );
}
