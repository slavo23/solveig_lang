use std::path::Path;
use std::process::Command;

use solveig_driver::checked_project_schemes;

#[test]
fn checker_api_golden_types() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/types/checker_api");
    let schemes = checked_project_schemes(&project_root)
        .expect("checker API golden fixture should resolve and check");

    let expected = [
        (
            "prelude.list.map",
            "∀ (a : *) (b : *). (a -> b) -> List a -> List b",
        ),
        (
            "prelude.list.foldr",
            "∀ (a : *) (b : *). (a -> b -> b) -> b -> List a -> b",
        ),
        (
            "prelude.typeclasses.notEq",
            "∀ (a : *). Eq a => a -> a -> Bool",
        ),
        (
            "prelude.typeclasses.less",
            "∀ (a : *). Ord a => a -> a -> Bool",
        ),
        (
            "main.compose",
            "∀ (a : *) (b : *) (c : *). (b -> c) -> (a -> b) -> a -> c",
        ),
        ("main.constLocal", "∀ (a : *) (b : *). a -> b -> a"),
        ("main.applyTwice", "∀ (a : *). (a -> a) -> a -> a"),
        ("main.useMap", "List Int -> List Int"),
        ("main.floatId", "Float -> Float"),
        ("main.charId", "Char -> Char"),
        ("main.stringId", "String -> String"),
        ("main.ioIntId", "IO Int -> IO Int"),
        ("main.sumViaFoldr", "List Int -> Int"),
        (
            "main.mapBox",
            "∀ (a : *) (b : *). (a -> b) -> Box a -> Box b",
        ),
        ("main.mapBoxInt", "Box Int -> Box Int"),
        (
            "main.sameWrap",
            "∀ (a : *). Eq a => Wrap a -> Wrap a -> Bool",
        ),
        ("main.notEqThrough", "∀ (a : *). Eq a => a -> a -> Bool"),
        ("main.lessViaOrd", "∀ (a : *). Ord a => a -> a -> Bool"),
        ("main.sameViaOrd", "∀ (a : *). Ord a => a -> a -> Bool"),
        ("main.lessInt", "Bool"),
        ("main.wrapNotEqInt", "Bool"),
        ("main.useRank2", "(∀ (a : *). a -> a) -> Int"),
        (
            "main.useRank2Eq",
            "(∀ (a : *). Eq a => a -> a -> Bool) -> Bool",
        ),
        ("main.polyId", "∀ (a : *). a -> a"),
        ("main.longElementId", "∀ (element : *). element -> element"),
        ("main.longElementResult", "Int"),
        (
            "main.shadowRank2",
            "∀ (a : *). (∀ (a : *). a -> a) -> a -> a",
        ),
        ("main.shadowRank2Result", "Int"),
        ("main.rank2Result", "Int"),
        ("main.rank2EqResult", "Bool"),
    ];

    for (name, expected_scheme) in expected {
        let actual = schemes
            .get(name)
            .unwrap_or_else(|| panic!("missing checked scheme for `{name}`"));

        assert_eq!(actual, expected_scheme, "scheme changed for `{name}`");
    }
}

#[test]
fn cli_does_not_see_unimported_instances() {
    let project_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/types/unimported_instance");
    let output = Command::new(env!("CARGO_BIN_EXE_solveig"))
        .arg("--project-root")
        .arg(&project_root)
        .arg("--verify-core")
        .output()
        .expect("Solveig CLI should run for unimported-instance fixture");
    let actual = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !output.status.success(),
        "unimported-instance fixture unexpectedly compiled:\n{actual}"
    );
    assert!(
        actual.contains("Need Int") && actual.contains("no matching instance found"),
        "unimported-instance fixture should fail because the sibling instance is not imported:\n\
         {actual}"
    );
}
