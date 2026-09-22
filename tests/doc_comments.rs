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
            &"An exact RFC 8259 number spelling.\n\nThe representation retains decimal digits and exponent text rather than\nrounding through Float. The representation is private: use the smart\nconstructors to preserve the invariant that every JsonNumber is valid.\nThis remains distinct from BigInt because JSON numbers may contain a\ndecimal fraction or exponent and decoding must retain their exact spelling."
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

    let http_project =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http_stream_client");
    let docs =
        solveig_driver::checked_project_docs(&http_project).expect("HTTP documentation checks");
    assert_eq!(
        docs.get("std.http.withResponse"),
        Some(
            &"Uses a response body as a scoped, demand-driven byte stream.\nThe default pull size is 32 KiB and the total streamed size is unbounded."
                .to_string(),
        )
    );
    assert!(
        docs.get("std.http.withResponseWith")
            .is_some_and(|text| text.contains("single-consumer response stream"))
    );
    assert!(
        docs.get("std.http.requestStream")
            .is_some_and(|text| text.contains("transport requests more input"))
    );
    assert!(
        docs.get("std.http.withStreamResponseWith")
            .is_some_and(|text| text.contains("Cleanup joins the upload owner thread"))
    );

    let tls_project = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls_client");
    let docs =
        solveig_driver::checked_project_docs(&tls_project).expect("TLS documentation checks");
    assert!(
        docs.get("std.tls.withTlsConnection")
            .is_some_and(|text| text.contains("Platform certificate trust"))
    );
    assert!(
        docs.get("std.tls.TlsTrust")
            .is_some_and(|text| text.contains("Neither policy disables"))
    );
    assert!(
        docs.get("std.tls.receiveTlsBytes")
            .is_some_and(|text| text.contains("truncated TLS"))
    );
}
