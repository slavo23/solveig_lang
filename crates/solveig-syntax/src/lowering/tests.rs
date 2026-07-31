use super::*;

fn lower_test_source(source: &str) -> ParsedModule {
    let session = Session::new();
    let file = session.add_file("test.sv", source);

    parse_source(&session, file, source)
}

#[test]
fn lowered_ast_nodes_point_into_cst_arena() {
    let parsed = lower_test_source("main : IO ()\nmain = do\n    printLine \"ok\"\n");
    let signature = &parsed.defs[0];
    let main_function = &parsed.defs[1];

    assert_eq!(parsed.cst.node(signature.2).kind(), SyntaxKind::Signature);
    assert_eq!(
        parsed.cst.node(main_function.2).kind(),
        SyntaxKind::FunctionDef
    );

    let SurfaceDefinition::FunDef { name, body, .. } = &main_function.0 else {
        panic!("expected main function definition");
    };
    assert_eq!(parsed.cst.node(name.2).kind(), SyntaxKind::FunctionDef);
    assert_eq!(parsed.cst.node(body.2).kind(), SyntaxKind::DoExpr);
}

#[test]
fn doc_comment_lowering_uses_explicit_cst_attachments() {
    let parsed = lower_test_source(
        "\
-- ordinary note
-- | Attached docs.
main = 1 -- ordinary trailing
trailing = 2 -- ^ Attached trailing docs.
",
    );

    let SurfaceDefinition::FunDef { docs, .. } = &parsed.defs[0].0 else {
        panic!("expected first definition to be a function");
    };
    assert_eq!(
        docs.as_ref().map(|doc| doc.text.as_str()),
        Some("Attached docs.")
    );

    let SurfaceDefinition::FunDef { docs, .. } = &parsed.defs[1].0 else {
        panic!("expected second definition to be a function");
    };
    assert_eq!(
        docs.as_ref().map(|doc| doc.text.as_str()),
        Some("Attached trailing docs.")
    );
}
