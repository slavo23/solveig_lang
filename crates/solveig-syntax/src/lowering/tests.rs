use super::*;

fn lower_test_source(source: &str) -> ParsedModule {
    let session = Session::new();
    let file = session.add_file("test.sv", source);

    parse_source(&session, file, source)
}

#[test]
fn modules_without_visibility_syntax_retain_empty_directives() {
    let parsed = lower_test_source("identity x = x\n");

    assert!(parsed.directives.exports.is_none());
    assert!(parsed.directives.private.is_empty());
    assert!(parsed.directives.reexports.is_empty());
}

#[test]
fn lowered_ast_nodes_point_into_cst_arena() {
    let parsed = lower_test_source("main : Task ()\nmain = do\n    printLine \"ok\"\n");
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
fn tuple_types_lower_as_constructor_applications_in_instance_heads() {
    let parsed = lower_test_source("instance Eq (a, b, c) where\n    (==) _ _ = True\n");
    let SurfaceDefinition::InstanceDef { args, .. } = &parsed.defs[0].0 else {
        panic!("expected instance definition");
    };
    let [tuple] = args.as_slice() else {
        panic!("expected one instance-head argument, got {args:?}");
    };

    let SurfaceTy::App(two_args, third) = &tuple.0 else {
        panic!("expected Tuple3 application, got {:?}", tuple.0);
    };
    assert!(matches!(third.0, SurfaceTy::Var(_)));
    let SurfaceTy::App(one_arg, second) = &two_args.0 else {
        panic!("expected second Tuple3 application, got {:?}", two_args.0);
    };
    assert!(matches!(second.0, SurfaceTy::Var(_)));
    let SurfaceTy::App(constructor, first) = &one_arg.0 else {
        panic!("expected first Tuple3 application, got {:?}", one_arg.0);
    };
    assert!(matches!(first.0, SurfaceTy::Var(_)));
    assert!(matches!(constructor.0, SurfaceTy::Con(_)));
}

#[test]
fn tuple_elements_accept_unparenthesized_constructor_patterns() {
    let parsed = lower_test_source("same (Atom a, Atom b) = a\n");
    let SurfaceDefinition::FunDef { params, .. } = &parsed.defs[0].0 else {
        panic!("expected function definition");
    };
    let [tuple] = params.as_slice() else {
        panic!("expected one tuple parameter, got {params:?}");
    };
    let SurfacePattern::Con(_, elements) = &tuple.0 else {
        panic!("expected tuple constructor pattern, got {:?}", tuple.0);
    };
    assert_eq!(elements.len(), 2);
    for element in elements {
        assert!(matches!(
            &element.0,
            SurfacePattern::Con(_, args)
                if args.len() == 1 && matches!(args[0].0, SurfacePattern::Var(_))
        ));
    }
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
