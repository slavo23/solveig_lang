mod entailment_tests {
    use super::*;
    use solveig_ast::ast::{InstanceId, ModuleId};

    const EQ: ClassId = ClassId(1);
    const ORD: ClassId = ClassId(2);
    const INT: TyConId = TyConId(10);
    const MAYBE: TyConId = TyConId(11);
    const LIST: TyConId = TyConId(12);
    const PAIR: TyConId = TyConId(13);

    fn tycon(id: TyConId) -> Ty {
        Ty::TyCon(id)
    }

    fn var(name: &str) -> Ty {
        let id = match name {
            "a" => TyVarId(0),
            "b" => TyVarId(1),
            other => panic!("ICE: unknown test type variable `{other}`"),
        };
        Ty::TyVar(id)
    }

    fn app(func: Ty, arg: Ty) -> Ty {
        Ty::make_app(func, arg)
    }

    fn maybe(arg: Ty) -> Ty {
        app(tycon(MAYBE), arg)
    }

    fn list(arg: Ty) -> Ty {
        app(tycon(LIST), arg)
    }

    fn pair(left: Ty, right: Ty) -> Ty {
        app(app(tycon(PAIR), left), right)
    }

    fn pred(class_id: ClassId, arg: Ty) -> Predicate {
        Predicate {
            class_id,
            args: vec![arg],
        }
    }

    fn instance_with_id(
        id: InstanceId,
        head: Predicate,
        context: Vec<Predicate>,
    ) -> InstanceDefinition<Span> {
        InstanceDefinition {
            id,
            span: 0..0,
            def_module: ModuleId(0),
            class_module: Some(ModuleId(0)),
            type_module: Some(ModuleId(0)),
            context,
            context_evidence: Vec::new(),
            head,
            superclass_evidence: Vec::new(),
            methods: Vec::new(),
        }
    }

    fn tiny_checker() -> TypeChecker {
        let a = var("a");
        let b = var("b");
        let instance_defs = vec![
            instance_with_id(InstanceId(0), pred(EQ, tycon(INT)), Vec::new()),
            instance_with_id(
                InstanceId(1),
                pred(EQ, maybe(a.clone())),
                vec![pred(EQ, a.clone())],
            ),
            instance_with_id(
                InstanceId(2),
                pred(EQ, list(a.clone())),
                vec![pred(EQ, a.clone())],
            ),
            instance_with_id(
                InstanceId(3),
                pred(EQ, pair(a.clone(), b.clone())),
                vec![pred(EQ, a), pred(EQ, b)],
            ),
        ];

        let env = TypeCheckEnv {
            names: Names::new(),
            constructor_types: FxHashMap::default(),
            func_schemes: FxHashMap::default(),
            class_defs: FxHashMap::from_iter([
                (
                    EQ,
                    ClassDefinition {
                        id: EQ,
                        name: "test.Eq".to_string(),
                        short_name: "Eq".to_string(),
                        params: vec![(TyVarId(0), Kind::Star)],
                        superclasses: Vec::new(),
                        methods: Vec::new(),
                    },
                ),
                (
                    ORD,
                    ClassDefinition {
                        id: ORD,
                        name: "test.Ord".to_string(),
                        short_name: "Ord".to_string(),
                        params: vec![(TyVarId(0), Kind::Star)],
                        superclasses: vec![pred(EQ, var("a"))],
                        methods: Vec::new(),
                    },
                ),
            ]),
            instance_defs,
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            tycon_kinds: FxHashMap::default(),
        };

        TypeChecker::with_env(&env, FileId(0))
    }

    #[test]
    fn entailment_returns_structural_evidence() {
        let checker = tiny_checker();
        let int = tycon(INT);
        let maybe_int = maybe(int.clone());
        let list_maybe_int = list(maybe_int.clone());
        let a = var("a");
        let given_ev = EvVarId(77);
        let missing = tycon(TyConId(99));

        let cases = vec![
            (
                "nested instance context",
                Vec::new(),
                pred(EQ, list_maybe_int),
                Ok(Ev::Inst(
                    InstanceId(2),
                    vec![maybe_int],
                    vec![Ev::Inst(
                        InstanceId(1),
                        vec![int],
                        vec![Ev::Inst(InstanceId(0), Vec::new(), Vec::new())],
                    )],
                )),
            ),
            (
                "superclass from given",
                vec![GivenEvidence {
                    ev: given_ev,
                    pred: pred(ORD, a.clone()),
                }],
                pred(EQ, a),
                Ok(Ev::Super(Box::new(Ev::Var(given_ev)), ORD, 0)),
            ),
            (
                "missing instance",
                Vec::new(),
                pred(EQ, missing),
                Err(pred(EQ, tycon(TyConId(99)))),
            ),
        ];

        for (name, givens, wanted, expected) in cases {
            let actual = checker.entail(&givens, &wanted).map_err(|err| err.pred);
            assert_eq!(actual, expected, "{name}");
        }
    }

    #[test]
    fn instance_table_overlap_detection_unifies_both_heads() {
        let table = InstanceTable::from_definitions(vec![
            instance_with_id(
                InstanceId(0),
                pred(EQ, pair(var("a"), tycon(INT))),
                Vec::new(),
            ),
            instance_with_id(
                InstanceId(1),
                pred(EQ, pair(tycon(PRIM_TYS.bool), var("b"))),
                Vec::new(),
            ),
        ]);

        let overlaps = table.overlaps();

        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].first, InstanceId(0));
        assert_eq!(overlaps[0].second, InstanceId(1));
        assert_eq!(overlaps[0].class, EQ);
    }

    #[test]
    fn wanted_solving_records_evidence_by_destination() {
        let mut checker = tiny_checker();
        let destinations = checker.add_wanteds(vec![pred(EQ, tycon(INT))], 0..1);

        let unresolved = checker.unresolved_wanteds();

        assert!(unresolved.is_empty());
        assert_eq!(
            checker.evidence_solution(destinations[0]),
            Some(&Ev::Inst(InstanceId(0), Vec::new(), Vec::new()))
        );
    }

    #[test]
    fn qualified_checking_wraps_body_in_evidence_abstraction() {
        let mut checker = tiny_checker();
        let expected = Ty::make_qualified(vec![pred(EQ, tycon(INT))], Ty::TyUnit);
        let expr = TermExprNode::new(TermExpr::Unit, 0..1);

        let (_, elaborated) = checker
            .solve_checking(Context::new(), &expr, &expected)
            .expect("qualified checking should succeed");

        match elaborated.node {
            TermExpr::EvAbs(_, actual_pred, body) => {
                assert_eq!(actual_pred, pred(EQ, tycon(INT)));
                assert!(matches!(body.node, TermExpr::Unit));
            }
            other => panic!("expected evidence abstraction, got {other:?}"),
        }
    }

    #[test]
    fn zonking_replaces_evidence_placeholders() {
        let mut checker = tiny_checker();
        let destinations = checker.add_wanteds(vec![pred(EQ, tycon(INT))], 0..1);
        let base = elab_node(TermExpr::Unit, 0..1, Ty::TyUnit);
        let expr = wrap_ev_apps(base, destinations, 0..1, &Ty::TyUnit);

        let unresolved = checker.unresolved_wanteds();
        assert!(unresolved.is_empty());

        let zonked = checker.zonk_expr(&expr);
        match zonked.node {
            TermExpr::EvApp(_, Ev::Inst(instance, ty_args, sub_evidence)) => {
                assert_eq!(instance, InstanceId(0));
                assert!(ty_args.is_empty());
                assert!(sub_evidence.is_empty());
            }
            other => panic!("expected solved instance evidence, got {other:?}"),
        }
    }

    #[test]
    fn global_scheme_instantiation_wraps_reference_in_placeholder_application() {
        let mut checker = tiny_checker();
        let def_id = DefId {
            module: ModuleId(9),
            name: solveig_base::Symbol(42),
        };
        checker.func_schemes.insert(
            def_id,
            Scheme {
                quantifiers: Vec::new(),
                predicates: vec![pred(EQ, tycon(INT))],
                ty: Ty::TyUnit,
            },
        );
        let expr = TermExprNode::new(TermExpr::Var(VarRef::Global(def_id)), 0..1);

        let (_, elaborated, inferred_ty) = checker
            .solve_inference(Context::new(), &expr)
            .expect("global instantiation should infer");

        assert_eq!(inferred_ty, Ty::TyUnit);
        match &elaborated.node {
            TermExpr::EvApp(inner, Ev::Placeholder(ev)) => {
                assert_eq!(checker.wanteds.len(), 1);
                assert_eq!(checker.wanteds[0].ev, *ev);
                assert!(matches!(inner.node, TermExpr::Var(VarRef::Global(id)) if id == def_id));
            }
            other => panic!("expected placeholder evidence application, got {other:?}"),
        }
    }

    #[test]
    fn subsumption_context_wanteds_wrap_checked_expression() {
        let mut checker = tiny_checker();
        let local_symbol = checker.names.symbol("localQualified");
        let local_name = "localQualified".to_string();
        let expr = TermExprNode::new(TermExpr::Var(VarRef::Local(local_symbol)), 0..1);
        let qualified_type = Ty::make_qualified(vec![pred(EQ, tycon(INT))], Ty::TyUnit);
        let ctx = Context::new().add_entry(ContextEntry::TypedVar {
            var_name: local_name,
            ty: qualified_type,
        });

        let (_, elaborated) = checker
            .solve_checking(ctx, &expr, &Ty::TyUnit)
            .expect("subsumption should discharge qualified left context");

        match &elaborated.node {
            TermExpr::EvApp(inner, Ev::Placeholder(ev)) => {
                assert_eq!(checker.wanteds.len(), 1);
                assert_eq!(checker.wanteds[0].ev, *ev);
                assert!(
                    matches!(inner.node, TermExpr::Var(VarRef::Local(symbol)) if symbol == local_symbol)
                );
            }
            other => panic!("expected subsumption evidence application, got {other:?}"),
        }
    }
}
