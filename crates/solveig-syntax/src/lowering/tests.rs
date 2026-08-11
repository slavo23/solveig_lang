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

#[test]
fn function_definitions_lower_pattern_parameters() {
    let parsed = lower_test_source("score (Face n) _ = n\n");
    let SurfaceDefinition::FunDef { params, .. } = &parsed.defs[0].0 else {
        panic!("expected function definition");
    };

    assert_eq!(params.len(), 2);
    assert!(matches!(
        &params[0].0,
        SurfacePattern::Con(name, args)
            if name.0.segments.len() == 1
                && args.len() == 1
                && matches!(args[0].0, SurfacePattern::Var(_))
    ));
    assert!(matches!(params[1].0, SurfacePattern::Wild));
}

#[test]
fn function_definition_pattern_parameters_stay_separate() {
    let parsed = lower_test_source("score _ total = total\ncount 0 x = x\n");

    let SurfaceDefinition::FunDef { params, .. } = &parsed.defs[0].0 else {
        panic!("expected first definition to be a function");
    };
    assert_eq!(params.len(), 2);
    assert!(matches!(params[0].0, SurfacePattern::Wild));
    assert!(matches!(params[1].0, SurfacePattern::Var(_)));

    let SurfaceDefinition::FunDef { params, .. } = &parsed.defs[1].0 else {
        panic!("expected second definition to be a function");
    };
    assert_eq!(params.len(), 2);
    assert!(matches!(params[0].0, SurfacePattern::Lit(_)));
    assert!(matches!(params[1].0, SurfacePattern::Var(_)));
}

#[test]
fn constructor_less_data_definition_lowers_with_no_constructors() {
    let parsed = lower_test_source("data Never\n");
    let SurfaceDefinition::DataDef {
        ty_params,
        constructors,
        ..
    } = &parsed.defs[0].0
    else {
        panic!("expected a data definition");
    };

    assert!(ty_params.is_empty());
    assert!(constructors.is_empty());
}
