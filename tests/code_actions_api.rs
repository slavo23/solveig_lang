use solveig_base::{PositionEncoding, Severity, SourcePosition};
use solveig_driver::{Compilation, SourceCodeAction};
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
    let fixed = apply_action(source, action);

    assert!(fixed.starts_with("answer : Int\nanswer = 42"));
    compilation.set_overlay(&source_path, fixed);
    let result = compilation.check(true).expect("fixed check should finish");
    assert!(result.success(), "{:#?}", result.diagnostics());

    let _ = fs::remove_dir_all(project_root);
}
