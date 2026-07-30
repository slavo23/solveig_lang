use std::path::Path;
use std::process::Command;

use solveig_driver::checked_project_schemes;

#[test]
fn checker_api_golden_types() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/types/checker_api");
    let schemes = checked_project_schemes(&project_root)
        .expect("checker API golden fixture should resolve and check");

    let expected = [
        ("prelude.list.map", "forall a b. (a -> b) -> [a] -> [b]"),
        (
            "prelude.list.foldr",
            "forall a b. (a -> b -> b) -> b -> [a] -> b",
        ),
        (
            "prelude.typeclasses.notEq",
            "forall a. Eq a => a -> a -> Bool",
        ),
        (
            "prelude.typeclasses.less",
            "forall a. Ord a => a -> a -> Bool",
        ),
        (
            "main.compose",
            "forall a b c. (b -> c) -> (a -> b) -> a -> c",
        ),
        ("main.constLocal", "forall a b. a -> b -> a"),
        ("main.applyTwice", "forall a. (a -> a) -> a -> a"),
        ("main.useMap", "[Int] -> [Int]"),
        ("main.floatId", "Float -> Float"),
        ("main.charId", "Char -> Char"),
        ("main.stringId", "String -> String"),
        ("main.ioIntId", "IO Int -> IO Int"),
        ("main.sumViaFoldr", "[Int] -> Int"),
        ("main.mapBox", "forall a b. (a -> b) -> Box a -> Box b"),
        ("main.mapBoxInt", "Box Int -> Box Int"),
        (
            "main.sameWrap",
            "forall a. Eq a => Wrap a -> Wrap a -> Bool",
        ),
        ("main.notEqThrough", "forall a. Eq a => a -> a -> Bool"),
        ("main.lessViaOrd", "forall a. Ord a => a -> a -> Bool"),
        ("main.sameViaOrd", "forall a. Ord a => a -> a -> Bool"),
        ("main.letUnderQualified", "forall a. Eq a => a -> Bool"),
        ("main.lessInt", "Bool"),
        ("main.wrapNotEqInt", "Bool"),
        ("main.useRank2", "(forall a. a -> a) -> Int"),
        (
            "main.useRank2Eq",
            "(forall a. Eq a => a -> a -> Bool) -> Bool",
        ),
        ("main.polyId", "forall a. a -> a"),
        ("main.longElementId", "forall element. element -> element"),
        ("main.longElementResult", "Int"),
        ("main.shadowRank2", "forall a. (forall b. b -> b) -> a -> a"),
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
fn inferred_constraint_golden_types() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/types/inferred");
    let schemes = checked_project_schemes(&project_root)
        .expect("inferred constrained fixture should resolve and check");

    let expected = [
        ("main.double", "forall a. Num a => a -> a"),
        ("main.lessAndSame", "forall a. Ord a => a -> a -> Bool"),
        ("main.containsLeft", "forall a. Eq a => a -> [a] -> Bool"),
        ("main.containsRight", "forall a. Eq a => a -> [a] -> Bool"),
        ("main.main", "Int"),
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
