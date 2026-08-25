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

fn write_single_package_manifest(root: &Path, name: &str) {
    write_source(
        &root.join("solveig.toml"),
        &format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[language]\nentry = \"main\"\n"
        ),
    );
}

#[test]
fn module_header_items_may_be_interleaved_before_declarations() {
    let project = support::temp_root("solveig_modules", "flexible_header_order");
    write_single_package_manifest(&project, "flexible_header");
    write_source(
        &project.join("src/core.sv"),
        r#"import std.list
export (value)
import std.text
private (hidden)

value : Int
value = 42

hidden : Int
hidden = 0
"#,
    );
    write_source(
        &project.join("src/main.sv"),
        "import flexible_header.core(value)\n\nmain : Int\nmain = value\n",
    );

    let mut compilation = Compilation::new(&project);
    let result = compilation
        .check(true)
        .expect("flexible module header project should check");
    assert!(result.success(), "{:#?}", result.diagnostics());
}

#[test]
fn imports_are_qualified_by_default_and_entity_selections_are_exact() {
    let project = support::temp_root("solveig_modules", "entity_imports");
    write_single_package_manifest(&project, "imports");
    write_source(
        &project.join("src/core.sv"),
        r#"export (Widget(..), value, (<+>), Describe)

data Widget = MkWidget Int

value : Int
value = 40

infixl 6 <+>
(<+>) : Int -> Int -> Int
(<+>) left right = left + right

class Describe a where
    describe : a -> String
"#,
    );
    write_source(
        &project.join("src/main.sv"),
        r#"import imports.core
import imports.core(value as answer, Widget(..) as Boxed, (<+>) as (<^>), Describe)

unwrap : Boxed -> Int
unwrap (MkWidget value) = value

unwrapQualified : core.Widget -> Int
unwrapQualified (core.MkWidget value) = value

describeAny : Describe a => a -> String
describeAny = describe

main : Int
main = unwrap (MkWidget answer) + unwrapQualified (imports.core.MkWidget 1) + (1 <^> 2)
"#,
    );

    let mut compilation = Compilation::new(&project);
    let result = compilation
        .check(true)
        .expect("entity-aware import project should check");
    assert!(
        result.success(),
        "qualified and selected imports should check: {:#?}",
        result.diagnostics()
    );
}

#[test]
fn bare_and_aliased_imports_do_not_leak_unqualified_or_suppressed_qualifiers() {
    let bare = support::temp_root("solveig_modules", "bare_import_scope");
    write_single_package_manifest(&bare, "bare_scope");
    write_source(&bare.join("src/core.sv"), "export (value)\n\nvalue = 41\n");
    write_source(
        &bare.join("src/main.sv"),
        "import bare_scope.core\n\nqualified = core.value\nmain = value\n",
    );
    let mut bare_compilation = Compilation::new(&bare);
    let bare_result = bare_compilation
        .check(true)
        .expect("bare import scope should diagnose an unqualified use");
    assert!(!bare_result.success());
    assert!(
        bare_result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "RES0003"),
        "{:#?}",
        bare_result.diagnostics()
    );

    let aliased = support::temp_root("solveig_modules", "aliased_import_scope");
    write_single_package_manifest(&aliased, "alias_scope");
    write_source(
        &aliased.join("src/core.sv"),
        "export (value)\n\nvalue = 41\n",
    );
    write_source(
        &aliased.join("src/main.sv"),
        "import alias_scope.core as Core\n\naliased = Core.value\nmain = alias_scope.core.value\n",
    );
    let mut alias_compilation = Compilation::new(&aliased);
    let alias_result = alias_compilation
        .check(true)
        .expect("suppressed qualifier should produce a diagnostic");
    assert!(!alias_result.success());
    assert!(
        alias_result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "RES0002"),
        "{:#?}",
        alias_result.diagnostics()
    );
}

#[test]
fn import_collisions_are_ambiguous_unless_they_resolve_to_the_same_definition() {
    let ambiguous = support::temp_root("solveig_modules", "ambiguous_imports");
    write_single_package_manifest(&ambiguous, "ambiguous");
    write_source(
        &ambiguous.join("src/left.sv"),
        "export (value)\n\nvalue = 1\n",
    );
    write_source(
        &ambiguous.join("src/right.sv"),
        "export (value)\n\nvalue = 2\n",
    );
    write_source(
        &ambiguous.join("src/main.sv"),
        "import ambiguous.left(value)\nimport ambiguous.right(value)\n\nmain = value\n",
    );
    let mut ambiguous_compilation = Compilation::new(&ambiguous);
    let ambiguous_result = ambiguous_compilation
        .check(true)
        .expect("ambiguous imports should produce a diagnostic");
    assert!(!ambiguous_result.success());
    assert!(
        ambiguous_result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "RES0001"),
        "{:#?}",
        ambiguous_result.diagnostics()
    );

    let shared = support::temp_root("solveig_modules", "shared_import_identity");
    write_single_package_manifest(&shared, "shared");
    write_source(
        &shared.join("src/origin.sv"),
        "export (value)\n\nvalue = 41\n",
    );
    write_source(
        &shared.join("src/facade.sv"),
        "reexport shared.origin(value)\n",
    );
    write_source(
        &shared.join("src/main.sv"),
        "import shared.origin(value)\nimport shared.facade(value)\n\nmain = value\n",
    );
    let mut shared_compilation = Compilation::new(&shared);
    let shared_result = shared_compilation
        .check(true)
        .expect("same-definition imports should check");
    assert!(
        shared_result.success(),
        "same DefId should not be ambiguous: {:#?}",
        shared_result.diagnostics()
    );
}

#[test]
fn colliding_default_module_qualifiers_are_diagnosed() {
    let project = support::temp_root("solveig_modules", "qualifier_collision");
    write_single_package_manifest(&project, "qualifiers");
    write_source(
        &project.join("src/left/tools.sv"),
        "export (left)\n\nleft = 1\n",
    );
    write_source(
        &project.join("src/right/tools.sv"),
        "export (right)\n\nright = 2\n",
    );
    write_source(
        &project.join("src/main.sv"),
        "import qualifiers.left.tools\nimport qualifiers.right.tools\n\nmain = 0\n",
    );

    let mut compilation = Compilation::new(&project);
    let result = compilation
        .check(true)
        .expect("qualifier collision should produce a diagnostic");
    assert!(!result.success());
    assert!(
        result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "RES0020"),
        "{:#?}",
        result.diagnostics()
    );
}

#[test]
fn star_imports_open_everything_and_precise_renames_hide_the_original_short_name() {
    let opened = support::temp_root("solveig_modules", "star_import");
    write_single_package_manifest(&opened, "opened");
    write_source(
        &opened.join("src/core.sv"),
        "export (first, second)\n\nfirst = 20\nsecond = 22\n",
    );
    write_source(
        &opened.join("src/main.sv"),
        "import opened.core(*)\n\nmain = first + second\n",
    );
    let mut opened_compilation = Compilation::new(&opened);
    let opened_result = opened_compilation
        .check(true)
        .expect("star import should check");
    assert!(
        opened_result.success(),
        "{:#?}",
        opened_result.diagnostics()
    );

    let precise = support::temp_root("solveig_modules", "precise_import");
    write_single_package_manifest(&precise, "precise");
    write_source(
        &precise.join("src/core.sv"),
        "export (value)\n\nvalue = 41\n",
    );
    write_source(
        &precise.join("src/main.sv"),
        "import precise.core(value as answer)\n\nqualified = core.value\nmain = answer + value\n",
    );
    let mut precise_compilation = Compilation::new(&precise);
    let precise_result = precise_compilation
        .check(true)
        .expect("hidden original short name should produce a diagnostic");
    assert!(!precise_result.success());
    assert!(
        precise_result
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "RES0003"),
        "{:#?}",
        precise_result.diagnostics()
    );
}

#[test]
fn constructors_and_methods_must_be_selected_through_their_owner() {
    let project = support::temp_root("solveig_modules", "owned_import_entities");
    write_single_package_manifest(&project, "owners");
    write_source(
        &project.join("src/core.sv"),
        r#"export (Widget(..), Describe)

data Widget = MkWidget Int

class Describe a where
    describe : a -> String
"#,
    );
    write_source(
        &project.join("src/main.sv"),
        "import owners.core(MkWidget, describe)\n\nmain = 0\n",
    );

    let mut compilation = Compilation::new(&project);
    let result = compilation
        .check(true)
        .expect("invalid member imports should produce diagnostics");
    assert!(!result.success());
    let owner_errors = result
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.code == "RES0019")
        .count();
    assert_eq!(owner_errors, 2, "{:#?}", result.diagnostics());
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
        r#"import library.facade(Widget(..), answer, plusOne, (<^>))

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
