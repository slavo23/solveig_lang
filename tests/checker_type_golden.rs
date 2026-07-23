use std::path::Path;

use solveig_lang::checked_project_schemes;

#[test]
fn checker_api_golden_types() {
    let project_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/types/checker_api");
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
        (
            "main.notEqThrough",
            "∀ (a : *). Eq a => a -> a -> Bool",
        ),
        (
            "main.lessViaOrd",
            "∀ (a : *). Ord a => a -> a -> Bool",
        ),
        (
            "main.sameViaOrd",
            "∀ (a : *). Ord a => a -> a -> Bool",
        ),
        ("main.lessInt", "Bool"),
        ("main.wrapNotEqInt", "Bool"),
        ("main.useRank2", "(∀ (a : *). a -> a) -> Int"),
        ("main.polyId", "∀ (a : *). a -> a"),
        ("main.rank2Result", "Int"),
    ];

    for (name, expected_scheme) in expected {
        let actual = schemes
            .get(name)
            .unwrap_or_else(|| panic!("missing checked scheme for `{name}`"));

        assert_eq!(actual, expected_scheme, "scheme changed for `{name}`");
    }
}
