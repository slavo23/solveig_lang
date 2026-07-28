use super::*;
use crate::inference::exhaustiveness::env::instantiate_ctor_fields;
use solveig_ast::ast::{
    CaseArm, ClassTable, ConstructorDef, CtorInfo, ExprNode, Kind, Names, TermExpr, TermExprNode,
    TyConDefinition, TyConInfo, TyConTable,
};
use solveig_base::Severity;

struct ExhaustivenessCase {
    name: &'static str,
    scrutinee_type: Ty,
    patterns: Vec<Pattern<Span>>,
    expected_missing: Vec<&'static str>,
}

#[test]
fn check_case_table_driven_witnesses() {
    let fixtures = test_constructor_fixtures();

    let cases = vec![
        ExhaustivenessCase {
            name: "bool_complete",
            scrutinee_type: bool_ty(),
            patterns: vec![con("True", vec![]), con("False", vec![])],
            expected_missing: vec![],
        },
        ExhaustivenessCase {
            name: "bool_missing_false",
            scrutinee_type: bool_ty(),
            patterns: vec![con("True", vec![])],
            expected_missing: vec!["False"],
        },
        ExhaustivenessCase {
            name: "maybe_nested_just_missing_false",
            scrutinee_type: maybe_ty(maybe_ty(bool_ty())),
            patterns: vec![
                con("Nothing", vec![]),
                con("Just", vec![con("Nothing", vec![])]),
                con("Just", vec![con("Just", vec![con("True", vec![])])]),
            ],
            expected_missing: vec!["Just (Just False)"],
        },
        ExhaustivenessCase {
            name: "list_missing_singleton",
            scrutinee_type: list_ty(int_ty()),
            patterns: vec![
                con("Nil", vec![]),
                con("Cons", vec![wild(), con("Cons", vec![wild(), wild()])]),
            ],
            expected_missing: vec!["_ :: []"],
        },
        ExhaustivenessCase {
            name: "int_literals_need_wildcard",
            scrutinee_type: int_ty(),
            patterns: vec![lit(1), lit(2)],
            expected_missing: vec!["_"],
        },
        ExhaustivenessCase {
            name: "pair_of_bools_missing_false_false",
            scrutinee_type: pair_ty(bool_ty(), bool_ty()),
            patterns: vec![
                con("MkPair", vec![con("True", vec![]), con("True", vec![])]),
                con("MkPair", vec![con("True", vec![]), con("False", vec![])]),
                con("MkPair", vec![con("False", vec![]), con("True", vec![])]),
            ],
            expected_missing: vec!["MkPair False False"],
        },
        ExhaustivenessCase {
            name: "wildcard_first_covers_all",
            scrutinee_type: maybe_ty(bool_ty()),
            patterns: vec![wild(), con("Nothing", vec![]), con("Just", vec![wild()])],
            expected_missing: vec![],
        },
    ];

    for case in cases {
        let actual = missing_patterns(
            &fixtures.names,
            &fixtures.ctor_table,
            &fixtures.tycon_table,
            &case.scrutinee_type,
            case.patterns,
        );

        assert_eq!(
            actual, case.expected_missing,
            "unexpected missing patterns for {}",
            case.name
        );
    }
}

#[test]
fn check_case_witnesses_use_constructor_table_names() {
    let tycon_id = TyConId(42);
    let hidden_id = CtorId(7);
    let shown_id = CtorId(9);
    let widget = Ty::TyCon(tycon_id);
    let mut names = Names::new();
    let data_def = DataDefinition {
        name: "test.pretty.Widget".to_string(),
        short_name: "Widget".to_string(),
        ty_params: Vec::new(),
        tycon_kind: Kind::Star,
        constructors: vec![
            ConstructorDef {
                id: hidden_id,
                name: "test.pretty.Hidden".to_string(),
                short_name: "Hidden".to_string(),
                fields: Vec::new(),
                full_type: widget.clone(),
            },
            ConstructorDef {
                id: shown_id,
                name: "test.pretty.Shown".to_string(),
                short_name: "Shown".to_string(),
                fields: Vec::new(),
                full_type: widget.clone(),
            },
        ],
    };

    let mut tycon_table = TyConTable::new();
    tycon_table.insert_with_id(
        tycon_id,
        TyConInfo {
            name: names.symbol("test.pretty.Widget"),
            short_name: names.symbol("Widget"),
            kind: Kind::Star,
            definition: TyConDefinition::Data(data_def),
        },
    );

    let mut ctor_table = CtorTable::new();
    ctor_table.insert_with_id(
        hidden_id,
        CtorInfo {
            name: names.symbol("test.pretty.Hidden"),
            short_name: names.symbol("Hidden"),
            tycon: tycon_id,
            fields: Vec::new(),
            full_type: widget.clone(),
            tag: 0,
        },
    );
    ctor_table.insert_with_id(
        shown_id,
        CtorInfo {
            name: names.symbol("test.pretty.Shown"),
            short_name: names.symbol("Shown"),
            tycon: tycon_id,
            fields: Vec::new(),
            full_type: widget.clone(),
            tag: 1,
        },
    );

    let class_table = ClassTable::new();
    let arms = vec![arm(Pattern::Con(hidden_id, Vec::new()))];

    let diagnostics = check_case(
        FileId(0),
        &tycon_table,
        &ctor_table,
        PrettyTables {
            names: &names,
            tycons: &tycon_table,
            classes: &class_table,
        },
        &widget,
        &arms,
        span(),
    );

    match diagnostics.error {
        Some(TypeError::NonExhaustivePatterns {
            scrutinee_type,
            missing,
            ..
        }) => {
            assert_eq!(scrutinee_type, "Widget");
            assert_eq!(missing, vec!["Shown"]);
        }
        Some(error) => panic!("ICE: expected only exhaustiveness errors, got {error:?}"),
        None => panic!("ICE: expected non-exhaustive pattern report"),
    }
}

#[test]
fn instantiate_ctor_fields_substitutes_scrutinee_type_args() {
    let fixtures = test_constructor_fixtures();
    let data_def = data_def_named(&fixtures.tycon_table, "List");
    let cons = fixtures
        .ctor_table
        .get(ctor_id("Cons"))
        .expect("ICE: test fixture must define Cons");

    let fields = instantiate_ctor_fields(data_def, cons, &[int_ty()]);

    assert_eq!(fields, vec![int_ty(), list_ty(int_ty())]);
}

#[test]
fn check_case_reports_unreachable_pattern_warnings() {
    let duplicate = redundancy_warnings_for_patterns(vec![
        con("True", vec![]),
        con("True", vec![]),
        con("False", vec![]),
    ]);

    assert_eq!(duplicate.len(), 1);
    assert_unreachable_warning(&duplicate[0]);

    let after_wildcard = redundancy_warnings_for_patterns(vec![wild(), con("True", vec![])]);

    assert_eq!(after_wildcard.len(), 1);
    assert_unreachable_warning(&after_wildcard[0]);

    let overlapping_but_useful = redundancy_warnings_for_maybe_bool_patterns(vec![
        con("Just", vec![con("True", vec![])]),
        con("Just", vec![wild()]),
        con("Nothing", vec![]),
    ]);

    assert!(overlapping_but_useful.is_empty());
}

struct TestConstructorFixtures {
    names: Names,
    ctor_table: CtorTable,
    tycon_table: TyConTable,
}

fn redundancy_warnings_for_patterns(patterns: Vec<Pattern<Span>>) -> Vec<TypeError> {
    check_patterns(bool_ty(), patterns).warnings
}

fn redundancy_warnings_for_maybe_bool_patterns(patterns: Vec<Pattern<Span>>) -> Vec<TypeError> {
    check_patterns(maybe_ty(bool_ty()), patterns).warnings
}

fn check_patterns(scrutinee_type: Ty, patterns: Vec<Pattern<Span>>) -> CaseDiagnostics {
    let fixtures = test_constructor_fixtures();
    let arms = patterns.into_iter().map(arm).collect::<Vec<_>>();
    let class_table = ClassTable::new();

    check_case(
        FileId(0),
        &fixtures.tycon_table,
        &fixtures.ctor_table,
        PrettyTables {
            names: &fixtures.names,
            tycons: &fixtures.tycon_table,
            classes: &class_table,
        },
        &scrutinee_type,
        &arms,
        span(),
    )
}

fn assert_unreachable_warning(error: &TypeError) {
    let TypeError::UnreachablePattern { .. } = error else {
        panic!("ICE: expected unreachable-pattern warning, got {error:?}");
    };

    let diagnostic = error.to_diagnostic();
    assert_eq!(diagnostic.code, "TC0017");
    assert_eq!(diagnostic.severity, Severity::Warning);
    assert_eq!(diagnostic.message, "unreachable pattern");
}

fn missing_patterns(
    names: &Names,
    ctor_table: &CtorTable,
    tycon_table: &TyConTable,
    scrutinee_type: &Ty,
    patterns: Vec<Pattern<Span>>,
) -> Vec<String> {
    let arms = patterns.into_iter().map(arm).collect::<Vec<_>>();
    let class_table = ClassTable::new();

    match check_case(
        FileId(0),
        tycon_table,
        ctor_table,
        PrettyTables {
            names,
            tycons: tycon_table,
            classes: &class_table,
        },
        scrutinee_type,
        &arms,
        span(),
    )
    .error
    {
        None => Vec::new(),
        Some(TypeError::NonExhaustivePatterns { missing, .. }) => missing,
        Some(error) => panic!("ICE: expected only exhaustiveness errors, got {error:?}"),
    }
}

fn test_constructor_fixtures() -> TestConstructorFixtures {
    let mut names = Names::new();
    let mut tycon_table = TyConTable::new();
    let mut ctor_table = CtorTable::new();

    tycon_table.insert_with_id(
        PRIM_TYS.int,
        TyConInfo {
            name: names.symbol("Int"),
            short_name: names.symbol("Int"),
            kind: Kind::Star,
            definition: TyConDefinition::Primitive,
        },
    );

    let data_defs = vec![
        DataDefinition {
            name: "Bool".to_string(),
            short_name: "Bool".to_string(),
            ty_params: Vec::new(),
            tycon_kind: Kind::Star,
            constructors: vec![
                ConstructorDef {
                    id: ctor_id("True"),
                    name: "True".to_string(),
                    short_name: "True".to_string(),
                    fields: Vec::new(),
                    full_type: bool_ty(),
                },
                ConstructorDef {
                    id: ctor_id("False"),
                    name: "False".to_string(),
                    short_name: "False".to_string(),
                    fields: Vec::new(),
                    full_type: bool_ty(),
                },
            ],
        },
        DataDefinition {
            name: "Maybe".to_string(),
            short_name: "Maybe".to_string(),
            ty_params: vec![(TyVarId(0), Kind::Star)],
            tycon_kind: unary_kind(),
            constructors: vec![
                ConstructorDef {
                    id: ctor_id("Nothing"),
                    name: "Nothing".to_string(),
                    short_name: "Nothing".to_string(),
                    fields: Vec::new(),
                    full_type: forall("a", maybe_ty(tyvar("a"))),
                },
                ConstructorDef {
                    id: ctor_id("Just"),
                    name: "Just".to_string(),
                    short_name: "Just".to_string(),
                    fields: vec![tyvar("a")],
                    full_type: forall("a", arr(tyvar("a"), maybe_ty(tyvar("a")))),
                },
            ],
        },
        DataDefinition {
            name: "List".to_string(),
            short_name: "List".to_string(),
            ty_params: vec![(TyVarId(0), Kind::Star)],
            tycon_kind: unary_kind(),
            constructors: vec![
                ConstructorDef {
                    id: ctor_id("Nil"),
                    name: "Nil".to_string(),
                    short_name: "Nil".to_string(),
                    fields: Vec::new(),
                    full_type: forall("a", list_ty(tyvar("a"))),
                },
                ConstructorDef {
                    id: ctor_id("Cons"),
                    name: "Cons".to_string(),
                    short_name: "Cons".to_string(),
                    fields: vec![tyvar("a"), list_ty(tyvar("a"))],
                    full_type: forall(
                        "a",
                        arr(tyvar("a"), arr(list_ty(tyvar("a")), list_ty(tyvar("a")))),
                    ),
                },
            ],
        },
        DataDefinition {
            name: "Pair".to_string(),
            short_name: "Pair".to_string(),
            ty_params: vec![(TyVarId(0), Kind::Star), (TyVarId(1), Kind::Star)],
            tycon_kind: binary_kind(),
            constructors: vec![ConstructorDef {
                id: ctor_id("MkPair"),
                name: "MkPair".to_string(),
                short_name: "MkPair".to_string(),
                fields: vec![tyvar("a"), tyvar("b")],
                full_type: forall(
                    "a",
                    forall(
                        "b",
                        arr(tyvar("a"), arr(tyvar("b"), pair_ty(tyvar("a"), tyvar("b")))),
                    ),
                ),
            }],
        },
    ];

    let mut next_tag = 0;
    for data_def in data_defs {
        insert_data_definition(
            &mut names,
            &mut tycon_table,
            &mut ctor_table,
            data_def,
            &mut next_tag,
        );
    }

    TestConstructorFixtures {
        names,
        ctor_table,
        tycon_table,
    }
}

fn insert_data_definition(
    names: &mut Names,
    tycon_table: &mut TyConTable,
    ctor_table: &mut CtorTable,
    data_def: DataDefinition,
    next_tag: &mut u16,
) {
    let tycon = tycon_id(&data_def.short_name);

    tycon_table.insert_with_id(
        tycon,
        TyConInfo {
            name: names.symbol(&data_def.name),
            short_name: names.symbol(&data_def.short_name),
            kind: data_def.tycon_kind.clone(),
            definition: TyConDefinition::Data(data_def.clone()),
        },
    );

    for constructor in &data_def.constructors {
        ctor_table.insert_with_id(
            constructor.id,
            CtorInfo {
                name: names.symbol(&constructor.name),
                short_name: names.symbol(&constructor.short_name),
                tycon,
                fields: constructor.fields.clone(),
                full_type: constructor.full_type.clone(),
                tag: *next_tag,
            },
        );

        *next_tag += 1;
    }
}

fn data_def_named<'a>(tycon_table: &'a TyConTable, name: &str) -> &'a DataDefinition {
    let info = tycon_table
        .get(tycon_id(name))
        .expect("ICE: test fixture must define requested tycon");

    let TyConDefinition::Data(data_def) = &info.definition else {
        panic!("ICE: test fixture requested data definition from primitive tycon");
    };

    data_def
}

fn arm(pattern: Pattern<Span>) -> CaseArmNode<Span> {
    ExprNode::new(
        CaseArm {
            pattern: pattern_node(pattern),
            expr: TermExprNode::new(TermExpr::Unit, span()),
        },
        span(),
    )
}

fn pattern_node(pattern: Pattern<Span>) -> PatternNode<Span> {
    ExprNode::new(pattern, span())
}

fn con(name: &str, args: Vec<Pattern<Span>>) -> Pattern<Span> {
    Pattern::Con(ctor_id(name), args.into_iter().map(pattern_node).collect())
}

fn wild() -> Pattern<Span> {
    Pattern::Wild
}

fn lit(value: i64) -> Pattern<Span> {
    Pattern::Lit(Literal::Int(value))
}

fn span() -> Span {
    0..0
}

fn bool_ty() -> Ty {
    tycon("Bool")
}

fn int_ty() -> Ty {
    tycon("Int")
}

fn unary_kind() -> Kind {
    Kind::make_arrow(Kind::Star, Kind::Star)
}

fn binary_kind() -> Kind {
    Kind::make_arrow(Kind::Star, unary_kind())
}

fn maybe_ty(arg: Ty) -> Ty {
    app(tycon("Maybe"), arg)
}

fn list_ty(arg: Ty) -> Ty {
    app(tycon("List"), arg)
}

fn pair_ty(left: Ty, right: Ty) -> Ty {
    app(app(tycon("Pair"), left), right)
}

fn tycon(name: &str) -> Ty {
    Ty::make_con_id(tycon_id(name))
}

fn tycon_id(name: &str) -> TyConId {
    match name {
        "Bool" => PRIM_TYS.bool,
        "Int" => PRIM_TYS.int,
        "Maybe" => TyConId(100),
        "List" => TyConId(101),
        "Pair" => TyConId(102),
        other => panic!("ICE: unknown test type constructor `{other}`"),
    }
}

fn ctor_id(name: &str) -> CtorId {
    match name {
        "True" => PRIM_TYS.true_ctor,
        "False" => PRIM_TYS.false_ctor,
        "Nothing" => CtorId(100),
        "Just" => CtorId(101),
        "Nil" => CtorId(102),
        "Cons" => CtorId(103),
        "MkPair" => CtorId(104),
        other => panic!("ICE: unknown test constructor `{other}`"),
    }
}

fn tyvar(name: &str) -> Ty {
    Ty::TyVar(test_tyvar_id(name))
}

fn app(func: Ty, arg: Ty) -> Ty {
    Ty::make_app(func, arg)
}

fn arr(arg: Ty, result: Ty) -> Ty {
    Ty::make_arr(arg, result)
}

fn forall(name: &str, body: Ty) -> Ty {
    Ty::make_forall_id(test_tyvar_id(name), Kind::Star, body)
}

fn test_tyvar_id(name: &str) -> TyVarId {
    match name {
        "a" => TyVarId(0),
        "b" => TyVarId(1),
        other => panic!("ICE: unknown test type variable `{other}`"),
    }
}
