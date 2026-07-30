use solveig_base::{PositionEncoding, Severity, SourcePosition};
use solveig_driver::{Compilation, SourceCodeAction, SourceCodeActionCategory};
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
        "solveig_code_actions_{name}_{}_{}",
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

fn action_named<'a>(actions: &'a [SourceCodeAction], title_prefix: &str) -> &'a SourceCodeAction {
    actions
        .iter()
        .find(|action| action.title().starts_with(title_prefix))
        .unwrap_or_else(|| panic!("missing action starting with {title_prefix:?}: {actions:#?}"))
}

fn apply_action(source: &str, action: &SourceCodeAction) -> String {
    let mut out = source.to_string();
    let mut edits = action.edits().to_vec();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.span().start));

    for edit in edits {
        out.replace_range(edit.span().to_usize_range(), edit.new_text());
    }

    out
}

fn offset_in_line(source: &str, line: &str, needle: &str) -> usize {
    let line_start = source
        .find(line)
        .unwrap_or_else(|| panic!("test source should contain line `{line}`"));
    let needle_offset = line
        .find(needle)
        .unwrap_or_else(|| panic!("line `{line}` should contain `{needle}`"));

    line_start + needle_offset
}

#[test]
fn typed_hole_actions_fill_suggested_constructor_and_recheck() {
    let project_root = temp_root("hole");
    let source_path = project_root.join("scratch.sv");
    let source = r#"main : Bool
main = ??
"#;
    write_source(&source_path, source);
    let mut compilation = Compilation::for_source_file(&source_path);

    let result = compilation.check(true).expect("check should finish");
    assert!(!result.success());
    assert!(
        result.diagnostics().iter().any(|diagnostic| {
            diagnostic.code == "HOLE" && diagnostic.severity == Severity::Note
        })
    );

    let actions = compilation.code_actions_at_range(
        &source_path,
        SourcePosition::new(1, 7),
        SourcePosition::new(1, 9),
        PositionEncoding::Utf16,
    );
    let action = action_named(&actions, "Fill hole with `True`");
    let fixed = apply_action(source, action);

    compilation.set_overlay(&source_path, fixed);
    let result = compilation.check(true).expect("fixed check should finish");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn missing_signature_action_handles_scry_mutual_recursion() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/scry");
    let source_path = project_root.join("src/scry/parser.sv");
    let source = fs::read_to_string(&source_path).expect("scry parser source should load");
    let edited = source.replacen("goalsFromSExps : [SExp] -> Maybe [Goal]\n", "", 1);
    let mut compilation = Compilation::new(&project_root);

    compilation.set_overlay(&source_path, edited.clone());
    let result = compilation
        .check(true)
        .expect("scry parser without goalsFromSExps signature should check");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let action = compilation
        .source_code_actions()
        .iter()
        .find(|action| action.title() == "Add type signature for `goalsFromSExps`")
        .unwrap_or_else(|| {
            panic!(
                "missing inferred signature action after recursive inference: {:#?}",
                compilation.source_code_actions()
            )
        });
    assert_eq!(
        action.category(),
        SourceCodeActionCategory::InferredSignature
    );

    let hover = compilation
        .hover_info(
            &source_path,
            offset_in_line(&edited, "goalsFromSExps sexps =", "goalsFromSExps"),
        )
        .expect("recursive function hover should exist without a written signature");
    assert_eq!(
        hover.signature(),
        "goalsFromSExps : List SExp -> Maybe (List Goal)"
    );
}

#[test]
fn missing_signature_action_handles_direct_recursion() {
    let project_root = temp_root("direct_recursion_signature");
    let source_path = project_root.join("scratch.sv");
    let source = r#"count xs =
    case xs of
        [] => 0
        | _ :: rest => 1 + count rest

main : Int
main = count [1, 2, 3]
"#;
    write_source(&source_path, source);
    let mut compilation = Compilation::for_source_file(&source_path);

    let result = compilation
        .check(true)
        .expect("direct recursive function without signature should check");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let action = compilation
        .source_code_actions()
        .iter()
        .find(|action| action.title() == "Add type signature for `count`")
        .unwrap_or_else(|| {
            panic!(
                "missing inferred signature action for direct recursion: {:#?}",
                compilation.source_code_actions()
            )
        });
    assert_eq!(
        action.category(),
        SourceCodeActionCategory::InferredSignature
    );

    let hover = compilation
        .hover_info(&source_path, offset_in_line(source, "count xs =", "count"))
        .expect("direct recursive function hover should exist without a written signature");
    assert_eq!(hover.signature(), "count : forall a. List a -> Int");

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn recursive_explicit_qualified_signature_still_checks() {
    let project_root = temp_root("explicit_qualified_recursion");
    let source_path = project_root.join("scratch.sv");
    let source = r#"contains : forall a. Eq a => a -> [a] -> Bool
contains target xs =
    case xs of
        [] => False
        | x :: rest => if x == target then True else contains target rest

main : Bool
main = contains 2 [1, 2, 3]
"#;
    write_source(&source_path, source);
    let mut compilation = Compilation::for_source_file(&source_path);

    let result = compilation
        .check(true)
        .expect("explicit qualified recursive function should check");
    assert!(result.success(), "{:#?}", result.diagnostics());
    assert!(
        compilation.source_code_actions().iter().all(|action| {
            action.title() != "Add type signature for `contains`"
                || action.category() != SourceCodeActionCategory::InferredSignature
        }),
        "explicit signatures should not receive inferred-signature actions: {:#?}",
        compilation.source_code_actions()
    );

    let hover = compilation
        .hover_info(
            &source_path,
            offset_in_line(source, "contains target xs =", "contains"),
        )
        .expect("explicit qualified recursive function hover should exist");
    assert_eq!(
        hover.signature(),
        "contains : forall a. Eq a => a -> [a] -> Bool"
    );

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn signatureless_recursive_function_solves_concrete_typeclass_evidence() {
    let project_root = temp_root("concrete_typeclass_recursion");
    let source_path = project_root.join("scratch.sv");
    let source = r#"hasZero xs =
    case xs of
        [] => False
        | x :: rest => if x == 0 then True else hasZero rest

main : Bool
main = hasZero [1, 2, 3]
"#;
    write_source(&source_path, source);
    let mut compilation = Compilation::for_source_file(&source_path);

    let result = compilation
        .check(true)
        .expect("signature-less recursive function with concrete typeclass evidence should check");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let action = compilation
        .source_code_actions()
        .iter()
        .find(|action| action.title() == "Add type signature for `hasZero`")
        .unwrap_or_else(|| {
            panic!(
                "missing inferred signature action for concrete typeclass recursion: {:#?}",
                compilation.source_code_actions()
            )
        });
    assert_eq!(
        action.category(),
        SourceCodeActionCategory::InferredSignature
    );

    let hover = compilation
        .hover_info(
            &source_path,
            offset_in_line(source, "hasZero xs =", "hasZero"),
        )
        .expect("concrete typeclass recursive function hover should exist");
    assert_eq!(hover.signature(), "hasZero : List Int -> Bool");

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn signatureless_recursive_function_infers_typeclass_constraint() {
    let project_root = temp_root("polymorphic_typeclass_recursion");
    let source_path = project_root.join("scratch.sv");
    let source = r#"containsPoly target xs =
    case xs of
        [] => False
        | x :: rest => if x == target then True else containsPoly target rest
"#;
    write_source(&source_path, source);
    let mut compilation = Compilation::for_source_file(&source_path);

    let result = compilation
        .check(true)
        .expect("polymorphic typeclass recursion should check");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let action = compilation
        .source_code_actions()
        .iter()
        .find(|action| action.title() == "Add type signature for `containsPoly`")
        .unwrap_or_else(|| {
            panic!(
                "missing inferred signature action for qualified recursion: {:#?}",
                compilation.source_code_actions()
            )
        });
    assert_eq!(
        action.category(),
        SourceCodeActionCategory::InferredSignature
    );

    let hover = compilation
        .hover_info(
            &source_path,
            offset_in_line(source, "containsPoly target xs =", "containsPoly"),
        )
        .expect("qualified recursive function hover should exist");
    assert!(
        hover.signature().contains("Eq ") && hover.signature().contains("=>"),
        "containsPoly should infer an Eq-qualified signature: {}",
        hover.signature()
    );
    assert!(
        hover.signature().contains(" -> List ") && hover.signature().ends_with(" -> Bool"),
        "containsPoly should infer a membership-like type: {}",
        hover.signature()
    );

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn missing_instance_method_action_inserts_typed_hole_stub() {
    let project_root = temp_root("instance_stub");
    let source_path = project_root.join("scratch.sv");
    let source = r#"class Describe a where
    describe : a -> String

data Box = MkBox Int

instance Describe Box where

main : Int
main = 1
"#;
    write_source(&source_path, source);
    let mut compilation = Compilation::for_source_file(&source_path);

    let result = compilation.check(true).expect("check should finish");
    assert!(result.diagnostics().iter().any(|diagnostic| {
        diagnostic.code == "TC0019" && diagnostic.severity == Severity::Error
    }));

    let actions = compilation.code_actions_at_range(
        &source_path,
        SourcePosition::new(5, 0),
        SourcePosition::new(5, 26),
        PositionEncoding::Utf16,
    );
    let action = action_named(&actions, "Add `describe` stub");
    let fixed = apply_action(source, action);

    compilation.set_overlay(&source_path, fixed);
    let result = compilation
        .check(true)
        .expect("stubbed check should finish");
    assert!(
        !result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error),
        "{:#?}",
        result.diagnostics()
    );
    assert!(
        result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "HOLE")
    );

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn missing_import_action_imports_unresolved_name_from_project_module() {
    let project_root = temp_root("missing_import");
    let main = project_root.join("src/main.sv");
    let lib = project_root.join("src/lib.sv");
    let source = r#"main : Int
main = answer
"#;
    write_source(
        &lib,
        r#"answer : Int
answer = 42
"#,
    );
    write_source(&main, source);
    let mut compilation = Compilation::new(&project_root);

    let result = compilation.check(true).expect("check should finish");
    assert!(result.diagnostics().iter().any(|diagnostic| {
        diagnostic.code == "RES0003" && diagnostic.severity == Severity::Error
    }));

    let actions = compilation.code_actions_at_range(
        &main,
        SourcePosition::new(1, 7),
        SourcePosition::new(1, 13),
        PositionEncoding::Utf16,
    );
    let action = action_named(&actions, "Import `answer` from `lib`");
    let fixed = apply_action(source, action);

    compilation.set_overlay(&main, fixed);
    let result = compilation.check(true).expect("fixed check should finish");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn selected_import_name_must_be_exported_by_module() {
    let project_root = temp_root("bad_selected_import");
    let main = project_root.join("src/main.sv");
    let lib = project_root.join("src/lib.sv");
    write_source(
        &lib,
        r#"answer : Int
answer = 42
"#,
    );
    write_source(
        &main,
        r#"import lib(answer, missingAnswer)

main : Int
main = answer
"#,
    );
    let mut compilation = Compilation::new(&project_root);

    let result = compilation.check(true).expect("check should finish");
    assert!(
        result.diagnostics().iter().any(|diagnostic| {
            diagnostic.code == "RES0014"
                && diagnostic.severity == Severity::Error
                && diagnostic.message.contains("missingAnswer")
        }),
        "missing selected import should be diagnosed: {:#?}",
        result.diagnostics()
    );

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn bundled_typeclasses_file_checks_when_opened_directly() {
    let source_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/solveig-driver/src/prelude/typeclasses.sv");
    let mut compilation = Compilation::for_source_file(&source_path);

    let result = compilation
        .check(true)
        .expect("direct prelude file check should finish");
    assert!(
        result.success(),
        "bundled prelude file should check as prelude.typeclasses, got: {:#?}",
        result.diagnostics()
    );
}

#[test]
fn inferred_signature_action_inserts_scheme_before_definition() {
    let project_root = temp_root("signature");
    let source_path = project_root.join("scratch.sv");
    let source = r#"answer = 42

main : Int
main = answer
"#;
    write_source(&source_path, source);
    let mut compilation = Compilation::for_source_file(&source_path);

    let result = compilation.check(true).expect("check should finish");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let actions = compilation.code_actions_at_range(
        &source_path,
        SourcePosition::new(0, 0),
        SourcePosition::new(0, 6),
        PositionEncoding::Utf16,
    );
    let action = action_named(&actions, "Add type signature for `answer`");
    assert_eq!(
        action.category(),
        SourceCodeActionCategory::InferredSignature
    );
    let fixed = apply_action(source, action);

    assert!(fixed.starts_with("answer : Int\nanswer = 42"));
    compilation.set_overlay(&source_path, fixed);
    let result = compilation.check(true).expect("fixed check should finish");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let _ = fs::remove_dir_all(project_root);
}

#[test]
fn inferred_signature_action_inserts_source_friendly_polymorphic_scheme() {
    let project_root = temp_root("polymorphic_signature");
    let source_path = project_root.join("scratch.sv");
    let source = r#"triple x y z = x + y + z

main : Int
main = triple 1 2 3
"#;
    write_source(&source_path, source);
    let mut compilation = Compilation::for_source_file(&source_path);

    let result = compilation.check(true).expect("check should finish");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let actions = compilation.code_actions_at_range(
        &source_path,
        SourcePosition::new(0, 0),
        SourcePosition::new(0, 6),
        PositionEncoding::Utf16,
    );
    let action = action_named(&actions, "Add type signature for `triple`");
    assert_eq!(
        action.category(),
        SourceCodeActionCategory::InferredSignature
    );
    let fixed = apply_action(source, action);

    assert!(
        fixed.starts_with("triple : forall a. Num a => a -> a -> a -> a\ntriple x y z = x + y + z"),
        "{fixed}"
    );
    assert!(!fixed.contains('∀'), "{fixed}");
    assert!(!fixed.contains(": *"), "{fixed}");

    compilation.set_overlay(&source_path, fixed);
    let result = compilation.check(true).expect("fixed check should finish");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let _ = fs::remove_dir_all(project_root);
}
