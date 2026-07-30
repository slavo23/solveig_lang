use std::path::Path;

#[test]
fn checked_project_docs_include_importable_declarations() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/doc_comments");
    let docs = solveig_driver::checked_project_docs(&project_root)
        .expect("doc comment fixture should check");

    assert_eq!(
        docs.get("docsLib.documentedIncrement"),
        Some(&"Adds one to an integer.".to_string())
    );
    assert_eq!(
        docs.get("docsLib.trailingValue"),
        Some(&"Comes from trailing docs.".to_string())
    );
    assert_eq!(
        docs.get("docsLib.Documented"),
        Some(&"Holds a documented integer.".to_string())
    );
    assert_eq!(
        docs.get("docsLib.MkDocumented"),
        Some(&"Wraps a documented integer.".to_string())
    );
    assert_eq!(
        docs.get("docsLib.Describe"),
        Some(&"Describes values.".to_string())
    );
    assert_eq!(
        docs.get("docsLib.describe"),
        Some(&"Converts a value into a friendly label.".to_string())
    );
    assert_eq!(
        docs.get("prelude.builtins.Int"),
        Some(
            &"Whole-number values used for counting, indexing, and integer arithmetic.".to_string()
        )
    );
    assert_eq!(
        docs.get("prelude.builtins.Maybe"),
        Some(&"A value that may be present or absent.".to_string())
    );
    assert_eq!(
        docs.get("prelude.list.Nil"),
        Some(&"The empty list.".to_string())
    );
    assert_eq!(
        docs.get("prelude.typeclasses.show"),
        Some(&"Renders a value as a string.".to_string())
    );
    assert_eq!(
        docs.get("prelude.prim.intCompare"),
        Some(&"Compares two integers and returns their ordering.".to_string())
    );
}
