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

    let parser_project =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/run/parser_config");
    let docs = solveig_driver::checked_project_docs(&parser_project)
        .expect("parser documentation fixture should check");

    assert_eq!(
        docs.get("std.parser.Parser"),
        Some(
            &"A compositional parser over an input type with caller-defined error values.\n\nThe constructor exposes the minimal functional representation needed by\nuser-defined primitives. Ordinary grammars should prefer the documented\ncombinators so consumption and streaming behavior stay predictable."
                .to_string()
        )
    );
    assert_eq!(
        docs.get("std.parser.choice"),
        Some(
            &"Tries the right parser only when the left parser fails without consuming.\n\nEmpty failures at the same position merge their expected items. Wrap the\nleft parser in attempt only when its consumed prefix is genuinely shared.\nFor example, alternatives for `\"ab\"` and `\"ac\"` need attempt around the\nfirst token sequence; alternatives starting with different tokens do not."
                .to_string()
        )
    );
    assert_eq!(
        docs.get("std.parser.<?>"),
        Some(
            &"Infix form of label, following the grammar-reading order `parser <?> name`.\n\nLike label, this changes only an empty failure. A consumed or cut failure\nkeeps its more specific diagnostic. The operator has precedence zero so a\ncomplete parser expression normally appears on its left without brackets."
                .to_string()
        )
    );
    assert_eq!(
        docs.get("std.parser.manyFold"),
        Some(
            &"Parses zero or more values and folds them without constructing a result list.\n\nAn empty failure ends the repetition. A consumed failure propagates. A\nsuccessful iteration that consumed nothing becomes a committed structural\nerror instead of looping forever."
                .to_string()
        )
    );
    assert_eq!(
        docs.get("std.parser.StreamInput"),
        Some(
            &"A persistent feedable token cursor for pure incremental parsing.\n\nThe vector retains all supplied tokens so attempt can restore checkpoints\nacross chunk boundaries. finishStream changes an empty cursor from Await to\nfinal End without performing IO inside the parser."
                .to_string()
        )
    );
    assert_eq!(
        docs.get("std.parser.renderError"),
        Some(
            &"Renders a structured error with deterministic expected-item ordering.\n\nThe custom renderer controls only caller-defined failures; position,\nunexpected input, expected items, contexts, and structural reasons keep the\nsame format for every parser."
                .to_string()
        )
    );
    assert!(
        docs.get("std.parser.parseStream")
            .is_some_and(|text| text.contains("Demand-drives a complete parser"))
    );

    let json_project = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/run/json");
    let docs = solveig_driver::checked_project_docs(&json_project)
        .expect("JSON documentation fixture should check");

    assert_eq!(
        docs.get("std.json.JsonNumber"),
        Some(
            &"An exact RFC 8259 number spelling.\n\nThe representation retains decimal digits and exponent text rather than\nrounding through Float. Use the smart constructors; renderJsonWith rejects\na directly forged invalid spelling. A future BigInt constructor can be\nadded without changing Json's number case."
                .to_string()
        )
    );
    assert_eq!(
        docs.get("std.json.Decoder"),
        Some(
            &"A JSON decoder with direct byte parsing and composable value decoding.\n\nDecoders operate on the same ByteInput parser semantics as Json values.\nPrimitive, array, and homogeneous-object decoders avoid allocating an\nintermediate Json tree. Heterogeneous record and tagged decoders reuse one\nbounded Json value at that composition boundary rather than adding another\nparser or forcing every Decoder through a tree."
                .to_string()
        )
    );
    assert_eq!(
        docs.get("std.json.decodeJson"),
        Some(
            &"Runs one direct decoder on complete bounded UTF-8 JSON bytes.\n\nLeading and trailing JSON whitespace are accepted. The returned\nParseReply retains structured position, expected-set, and custom errors."
                .to_string()
        )
    );
    assert!(
        docs.get("std.json.Encoder")
            .is_some_and(|text| text.contains("symmetric value encoder"))
    );
    assert!(
        docs.get("std.json.requiredField")
            .is_some_and(|text| text.contains("field's path"))
    );
    assert!(
        docs.get("std.json.jsonNumberToInt")
            .is_some_and(|text| text.contains("No Float conversion"))
    );
}
