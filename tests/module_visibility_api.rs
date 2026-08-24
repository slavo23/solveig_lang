use std::fs;
use std::path::Path;

use solveig_driver::Compilation;
use solveig_ide::CompilationIdeExt;

use crate::support;

fn write_source(path: &Path, source: &str) {
    fs::create_dir_all(path.parent().expect("test source must have a parent"))
        .expect("test source directory should be creatable");
    fs::write(path, source).expect("test source should be writable");
}

#[test]
fn typed_values_and_open_constructors_survive_a_renamed_reexport() {
    let container = support::temp_root("solveig_modules", "typed_reexport");
    let app = container.join("app");
    let library = container.join("library");

    write_source(
        &app.join("solveig.toml"),
        r#"[package]
name = "app"
version = "0.1.0"

[dependencies]
library = { path = "../library" }

[language]
entry = "main"
"#,
    );
    write_source(
        &library.join("solveig.toml"),
        r#"[package]
name = "library"
version = "0.1.0"

[language]
entry = "facade"
"#,
    );
    let facade_source = r#"export (plusOne)
reexport library.origin(Widget(..), value as answer, (<+>) as (<^>))
import library.origin(value)

plusOne : Int
plusOne = value + 1
"#;
    let facade = library.join("src/facade.sv");
    let origin = library.join("src/origin.sv");
    write_source(
        &library.join("src/origin.sv"),
        r#"export (Widget(..), value, (<+>))

data Widget = MkWidget Int

value : Int
value = 41

infixl 6 <+>
(<+>) : Int -> Int -> Int
(<+>) left right = left + right
"#,
    );
    write_source(&facade, facade_source);
    write_source(
        &app.join("src/main.sv"),
        r#"import library.facade(Widget, MkWidget, answer, plusOne, (<^>))

unwrap : Widget -> Int
unwrap (MkWidget value) = value

main : Int
main = unwrap (MkWidget answer) + plusOne + (1 <^> 2)
"#,
    );
    let mut compilation = Compilation::new(&app);
    let result = compilation
        .check(true)
        .expect("module project should check");
    assert!(
        result.success(),
        "renamed typed re-export should check: {:#?}",
        result.diagnostics()
    );

    let alias_offset = facade_source
        .find("answer")
        .expect("re-export fixture should contain its alias")
        + 1;
    let facade = fs::canonicalize(facade).expect("facade source should canonicalize");
    let definition = compilation
        .definition(&facade, alias_offset)
        .expect("re-export alias should navigate to its source definition");
    assert_eq!(
        compilation.source_map().lookup(definition.file).path,
        fs::canonicalize(origin).expect("origin source should canonicalize")
    );
    let hover = compilation
        .hover_info(&facade, alias_offset)
        .expect("re-export alias should retain typed hover information");
    assert_eq!(hover.signature(), "value : Int");
}

#[test]
fn reexport_does_not_introduce_an_unqualified_local_binding() {
    let project = support::temp_root("solveig_modules", "reexport_local_scope");
    write_source(
        &project.join("solveig.toml"),
        r#"[package]
name = "scope"
version = "0.1.0"

[language]
entry = "main"
"#,
    );
    write_source(
        &project.join("src/source.sv"),
        "export (value)\n\nvalue = 41\n",
    );
    write_source(
        &project.join("src/main.sv"),
        "reexport scope.source(value as answer)\n\nmain = answer\n",
    );

    let mut compilation = Compilation::new(&project);
    let result = compilation
        .check(true)
        .expect("invalid local re-export use should produce diagnostics");

    assert!(!result.success());
    assert!(
        result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "RES0003"),
        "{:#?}",
        result.diagnostics()
    );
}

#[test]
fn dependency_imports_cannot_cross_opaque_or_unexported_boundaries() {
    let container = support::temp_root("solveig_modules", "external_boundaries");
    let app = container.join("app");
    let library = container.join("library");

    write_source(
        &app.join("solveig.toml"),
        r#"[package]
name = "app"
version = "0.1.0"

[dependencies]
library = { path = "../library" }

[language]
entry = "main"
"#,
    );
    write_source(
        &library.join("solveig.toml"),
        r#"[package]
name = "library"
version = "0.1.0"

[language]
entry = "api"
"#,
    );
    write_source(
        &library.join("src/api.sv"),
        r#"export (Opaque, public)
private (secret)

data Opaque = MkOpaque Int
public = 1
internal = 2
secret = 3
"#,
    );
    write_source(
        &app.join("src/main.sv"),
        r#"import library.api(public, internal, secret, MkOpaque)

main = public
"#,
    );
    write_source(
        &library.join("src/sibling.sv"),
        "import api(secret)\n\nsibling = secret\n",
    );

    let mut compilation = Compilation::new(&app);
    let result = compilation
        .check(true)
        .expect("boundary project should produce diagnostics");
    assert!(!result.success());
    let unknown_exports = result
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.code == "RES0014")
        .count();
    assert_eq!(unknown_exports, 4, "{:#?}", result.diagnostics());
}
