impl TypeChecker {
    fn free_type_vars_in_type(ty: &TypeExpr, out: &mut BTreeSet<TyVarId>) {
        match ty {
            TypeExpr::TyVar(id) => {
                out.insert(*id);
            }

            TypeExpr::TyArr(lhs, rhs) | TypeExpr::TyApp(lhs, rhs) => {
                Self::free_type_vars_in_type(lhs, out);
                Self::free_type_vars_in_type(rhs, out);
            }

            TypeExpr::TyForAll(binders, preds, body) => {
                for pred in preds {
                    Self::free_type_vars_in_predicate(pred, out);
                }
                Self::free_type_vars_in_type(body, out);
                for (name, _) in binders {
                    out.remove(name);
                }
            }

            TypeExpr::TyUnit | TypeExpr::TyCon(_) | TypeExpr::TyExVar(_) => {}
        }
    }
    fn free_type_vars_in_predicate(pred: &Predicate, out: &mut BTreeSet<TyVarId>) {
        for arg in &pred.args {
            Self::free_type_vars_in_type(arg, out);
        }
    }
    fn free_type_vars_in_instance(inst: &InstanceDefinition<Span>) -> Vec<TyVarId> {
        let mut out = BTreeSet::new();

        for pred in &inst.context {
            Self::free_type_vars_in_predicate(pred, &mut out);
        }

        Self::free_type_vars_in_predicate(&inst.head, &mut out);

        out.into_iter().collect()
    }
    fn instance_quantifiers(inst: &InstanceDefinition<Span>) -> Vec<(TyVarId, Kind)> {
        Self::free_type_vars_in_instance(inst)
            .into_iter()
            .map(|name| (name, Kind::Star))
            .collect()
    }
    fn instantiate_predicate_with_subst(
        pred: &Predicate,
        subst: &HashMap<TyVarId, TypeExpr>,
    ) -> Predicate {
        Predicate {
            class_id: pred.class_id,
            args: pred
                .args
                .iter()
                .map(|arg| {
                    subst
                        .iter()
                        .fold(arg.clone(), |acc, (name, ty)| acc.substitute(*name, ty))
                })
                .collect(),
        }
    }
    fn instantiate_scheme(
        &mut self,
        ctx: Context,
        scheme: &Scheme,
    ) -> (Context, Vec<Predicate>, TypeExpr) {
        let mut ctx = ctx;
        let mut subst = HashMap::<TyVarId, TypeExpr>::new();

        for (name, kind) in &scheme.quantifiers {
            let fresh = self.fresh_evar_id();
            ctx = ctx.add_entry(ContextEntry::ExVar(fresh, kind.clone()));
            subst.insert(*name, Self::evar_type(fresh));
        }

        let instantiated_preds = scheme
            .predicates
            .iter()
            .map(|pred| Self::instantiate_predicate_with_subst(pred, &subst))
            .collect::<Vec<_>>();

        let instantiated_ty = subst.iter().fold(scheme.ty.clone(), |acc, (name, ty)| {
            acc.substitute(*name, ty)
        });

        (ctx, instantiated_preds, instantiated_ty)
    }
    fn add_wanteds(&mut self, preds: Vec<Predicate>, span: Span) -> Vec<EvVarId> {
        let givens = self
            .givens
            .iter()
            .map(|given| GivenEvidence {
                ev: given.ev,
                pred: self.zonk_predicate(&given.pred),
            })
            .collect::<Vec<_>>();

        let mut evvars = Vec::new();
        let mut zonked = Vec::new();

        for pred in preds {
            let ev = self.fresh_evvar_id();
            evvars.push(ev);
            zonked.push(Wanted {
                ev,
                pred: self.zonk_predicate(&pred),
                span: span.clone(),
                givens: givens.clone(),
            });
        }

        self.wanteds.extend(zonked);
        evvars
    }
    fn with_givens<R>(
        &mut self,
        preds: Vec<Predicate>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> (Vec<GivenEvidence>, R) {
        let old_len = self.givens.len();

        let new_givens = preds
            .into_iter()
            .map(|pred| GivenEvidence {
                ev: self.fresh_evvar_id(),
                pred: self.zonk_predicate(&pred),
            })
            .collect::<Vec<_>>();

        self.givens.extend(new_givens.clone());
        let result = f(self);
        self.givens.truncate(old_len);
        (new_givens, result)
    }
    fn substitute_predicate(pred: &Predicate, subst: &HashMap<TyVarId, TypeExpr>) -> Predicate {
        Predicate {
            class_id: pred.class_id,
            args: pred
                .args
                .iter()
                .map(|arg| {
                    subst
                        .iter()
                        .fold(arg.clone(), |acc, (name, ty)| acc.substitute(*name, ty))
                })
                .collect(),
        }
    }
    fn substitute_scheme(scheme: &Scheme, subst: &HashMap<TyVarId, TypeExpr>) -> Scheme {
        Scheme {
            quantifiers: scheme.quantifiers.clone(),

            predicates: scheme
                .predicates
                .iter()
                .map(|pred| Self::substitute_predicate(pred, subst))
                .collect(),

            ty: subst.iter().fold(scheme.ty.clone(), |acc, (name, ty)| {
                acc.substitute(*name, ty)
            }),
        }
    }
    fn predicate_same_shape(a: &Predicate, b: &Predicate) -> bool {
        a.class_id == b.class_id && a.args == b.args
    }
    fn superclass_predicates_for_given(&self, given: &Predicate) -> Vec<Predicate> {
        let given = self.zonk_predicate(given);

        let Some(class_def) = self.class_defs.get(&given.class_id) else {
            return Vec::new();
        };

        if class_def.params.len() != given.args.len() {
            return Vec::new();
        }

        let subst = class_def
            .params
            .iter()
            .zip(given.args.iter())
            .map(|((param_name, _), arg)| (legacy_tyvar_id(param_name), arg.clone()))
            .collect::<HashMap<_, _>>();

        class_def
            .superclasses
            .iter()
            .map(|pred| Self::instantiate_predicate_with_subst(pred, &subst))
            .map(|pred| self.zonk_predicate(&pred))
            .collect()
    }
    fn specialize_superclasses_for_instance(
        class_def: &ClassDefinition,
        instance_head: &Predicate,
    ) -> Vec<Predicate> {
        let subst = class_def
            .params
            .iter()
            .zip(instance_head.args.iter())
            .map(|((param_name, _), instance_arg)| {
                (legacy_tyvar_id(param_name), instance_arg.clone())
            })
            .collect::<HashMap<_, _>>();

        class_def
            .superclasses
            .iter()
            .map(|pred| Self::instantiate_predicate_with_subst(pred, &subst))
            .collect()
    }
    fn specialize_class_method_for_instance(
        class_def: &ClassDefinition,
        method_scheme: &Scheme,
        instance_head: &Predicate,
        instance_context: &[Predicate],
        instance_quantifiers: &[(TyVarId, Kind)],
    ) -> Scheme {
        let mut subst = HashMap::<TyVarId, TypeExpr>::new();

        for ((param_name, _), instance_arg) in
            class_def.params.iter().zip(instance_head.args.iter())
        {
            subst.insert(legacy_tyvar_id(param_name), instance_arg.clone());
        }

        let specialized = Self::substitute_scheme(method_scheme, &subst);

        let specialized_class_head = Predicate {
            class_id: class_def.id,
            args: class_def
                .params
                .iter()
                .map(|(name, _)| {
                    subst
                        .get(&legacy_tyvar_id(name))
                        .cloned()
                        .unwrap_or_else(|| TypeExpr::TyVar(legacy_tyvar_id(name)))
                })
                .collect(),
        };

        let mut predicates = specialized
            .predicates
            .into_iter()
            .filter(|pred| !Self::predicate_same_shape(pred, &specialized_class_head))
            .collect::<Vec<_>>();

        predicates.extend(instance_context.iter().cloned());

        let mut quantifiers = instance_quantifiers.to_vec();

        for (id, kind) in specialized.quantifiers {
            if !quantifiers.iter().any(|(existing, _)| existing == &id) {
                quantifiers.push((id, kind));
            }
        }

        Scheme {
            quantifiers,
            predicates,
            ty: specialized.ty,
        }
    }
    fn check_instance_overlaps(&self) -> Vec<TypeError> {
        self.instance_table
            .overlaps()
            .into_iter()
            .map(|overlap| TypeError::OverlappingInstances {
                span: self.get_file_span(0..0),
                class_name: self.display_class_name(overlap.class),
                instance_type: overlap
                    .instance_args
                    .iter()
                    .map(|arg| self.pretty_type(arg))
                    .collect::<Vec<_>>()
                    .join(" "),
            })
            .collect()
    }
    fn match_instance_type(
        pattern: &TypeExpr,
        target: &TypeExpr,
        subst: &mut HashMap<TyVarId, TypeExpr>,
    ) -> bool {
        match pattern {
            TypeExpr::TyVar(id) => match subst.get(id) {
                Some(existing) => existing == target,
                None => {
                    subst.insert(*id, target.clone());
                    true
                }
            },

            TypeExpr::TyCon(a) => matches!(target, TypeExpr::TyCon(b) if a == b),
            TypeExpr::TyUnit => matches!(target, TypeExpr::TyUnit),

            TypeExpr::TyApp(f1, a1) => match target {
                TypeExpr::TyApp(f2, a2) => {
                    Self::match_instance_type(f1, f2, subst)
                        && Self::match_instance_type(a1, a2, subst)
                }
                _ => false,
            },

            TypeExpr::TyArr(a1, b1) => match target {
                TypeExpr::TyArr(a2, b2) => {
                    Self::match_instance_type(a1, a2, subst)
                        && Self::match_instance_type(b1, b2, subst)
                }
                _ => false,
            },

            TypeExpr::TyForAll(_, _, body) => Self::match_instance_type(body, target, subst),

            TypeExpr::TyExVar(name) => matches!(target, TypeExpr::TyExVar(other) if name == other),
        }
    }
    fn match_predicate_pattern(
        pattern: &Predicate,
        target: &Predicate,
    ) -> Option<HashMap<TyVarId, TypeExpr>> {
        if pattern.class_id != target.class_id || pattern.args.len() != target.args.len() {
            return None;
        }

        let mut subst = HashMap::<TyVarId, TypeExpr>::new();

        for (pattern_arg, target_arg) in pattern.args.iter().zip(target.args.iter()) {
            if !Self::match_instance_type(pattern_arg, target_arg, &mut subst) {
                return None;
            }
        }

        Some(subst)
    }
    fn evidence_ty_args_for_instance(
        instance_head: &Predicate,
        instance_context: &[Predicate],
        subst: &HashMap<TyVarId, TypeExpr>,
    ) -> Vec<TypeExpr> {
        let mut vars = BTreeSet::new();

        for pred in instance_context {
            Self::free_type_vars_in_predicate(pred, &mut vars);
        }

        Self::free_type_vars_in_predicate(instance_head, &mut vars);

        vars.into_iter()
            .map(|id| subst.get(&id).cloned().unwrap_or(TypeExpr::TyVar(id)))
            .collect()
    }
    fn discharge_predicate_with_givens(&mut self, pred: &Predicate, givens: &[Predicate]) -> bool {
        let mut evidence_givens = Vec::new();

        for pred in givens {
            evidence_givens.push(GivenEvidence {
                ev: self.fresh_evvar_id(),
                pred: self.zonk_predicate(pred),
            });
        }

        self.entail(&evidence_givens, pred).is_ok()
    }
    // THIH §7.3 defines entailment as superclass closure over givens followed
    // by instance matching (`bySuper` then `byInst`). C1 keeps the same search
    // order and attaches evidence to each successful derivation step.
    fn entail(&self, givens: &[GivenEvidence], wanted: &Predicate) -> Result<Ev, Unsolved> {
        self.entail_inner(givens, wanted, &mut HashSet::new())
    }
    fn entail_inner(
        &self,
        givens: &[GivenEvidence],
        wanted: &Predicate,
        seen: &mut HashSet<Predicate>,
    ) -> Result<Ev, Unsolved> {
        let wanted = self.zonk_predicate(wanted);

        for given in givens {
            if self.zonk_predicate(&given.pred) == wanted {
                return Ok(Ev::Var(given.ev));
            }
        }

        for given in givens.iter().rev() {
            if let Some(evidence) =
                self.superclass_evidence_from_given(given, &wanted, &mut HashSet::new())
            {
                return Ok(evidence);
            }
        }

        if !seen.insert(wanted.clone()) {
            return Err(Unsolved { pred: wanted });
        }

        for (instance_id, inst) in self.instance_table.iter() {
            if inst.class != wanted.class_id || inst.head.args.len() != wanted.args.len() {
                continue;
            }

            let Some(subst) = Self::match_predicate_pattern(&inst.head, &wanted) else {
                continue;
            };

            let mut sub_evidence = Vec::new();
            let mut solved_context = true;

            for ctx_pred in &inst.context {
                let instantiated = Self::instantiate_predicate_with_subst(ctx_pred, &subst);
                match self.entail_inner(givens, &instantiated, seen) {
                    Ok(evidence) => sub_evidence.push(evidence),
                    Err(_) => {
                        solved_context = false;
                        break;
                    }
                }
            }

            if solved_context {
                seen.remove(&wanted);
                return Ok(Ev::Inst(
                    instance_id,
                    Self::evidence_ty_args_for_instance(&inst.head, &inst.context, &subst),
                    sub_evidence,
                ));
            }
        }

        seen.remove(&wanted);
        Err(Unsolved { pred: wanted })
    }
    fn superclass_evidence_from_given(
        &self,
        given: &GivenEvidence,
        wanted: &Predicate,
        seen: &mut HashSet<Predicate>,
    ) -> Option<Ev> {
        self.superclass_evidence_from_predicate(
            self.zonk_predicate(&given.pred),
            Ev::Var(given.ev),
            wanted,
            seen,
        )
    }
    fn superclass_evidence_from_predicate(
        &self,
        source: Predicate,
        source_ev: Ev,
        wanted: &Predicate,
        seen: &mut HashSet<Predicate>,
    ) -> Option<Ev> {
        let source = self.zonk_predicate(&source);
        let wanted = self.zonk_predicate(wanted);

        if source == wanted {
            return Some(source_ev);
        }

        if !seen.insert(source.clone()) {
            return None;
        }

        for (index, superclass_pred) in self
            .superclass_predicates_for_given(&source)
            .into_iter()
            .enumerate()
        {
            let superclass_ev = Ev::Super(Box::new(source_ev.clone()), source.class_id, index);

            if let Some(evidence) = self.superclass_evidence_from_predicate(
                superclass_pred,
                superclass_ev,
                &wanted,
                seen,
            ) {
                seen.remove(&source);
                return Some(evidence);
            }
        }

        seen.remove(&source);
        None
    }
    fn unresolved_wanteds(&mut self) -> Vec<Wanted> {
        let mut unresolved = Vec::new();

        for wanted in self.wanteds.clone() {
            match self.entail(&wanted.givens, &wanted.pred) {
                Ok(evidence) => {
                    self.evidence_solutions.insert(wanted.ev, evidence);
                }
                Err(_) => unresolved.push(wanted),
            }
        }

        unresolved
    }
    fn wanteds_error(&self, wanteds: Vec<Wanted>) -> TypeError {
        let span = wanteds
            .first()
            .map(|w| self.get_file_span(w.span.clone()))
            .unwrap_or_else(|| self.get_file_span(0..0));

        TypeError::UnsatisfiedConstraints {
            span,
            predicates: wanteds
                .into_iter()
                .map(|w| self.pretty_predicate(&self.zonk_predicate(&w.pred)))
                .collect(),
        }
    }
    fn collect_exvars_in_type(ty: &TypeExpr, acc: &mut Vec<EvarId>) {
        match ty {
            TypeExpr::TyExVar(id) => {
                if !acc.contains(id) {
                    acc.push(*id);
                }
            }
            TypeExpr::TyArr(lhs, rhs) | TypeExpr::TyApp(lhs, rhs) => {
                Self::collect_exvars_in_type(lhs, acc);
                Self::collect_exvars_in_type(rhs, acc);
            }
            TypeExpr::TyForAll(_, preds, body) => {
                for pred in preds {
                    for arg in &pred.args {
                        Self::collect_exvars_in_type(arg, acc);
                    }
                }
                Self::collect_exvars_in_type(body, acc);
            }
            TypeExpr::TyUnit | TypeExpr::TyVar(_) | TypeExpr::TyCon(_) => {}
        }
    }
    fn generalize_top_level_type(&mut self, ctx: &Context, ty: &TypeExpr) -> TypeExpr {
        let zonked = self.zonk_type(&ctx.apply(ty));

        let mut evars = Vec::new();
        Self::collect_exvars_in_type(&zonked, &mut evars);

        if evars.is_empty() {
            return zonked;
        }

        let mut result = zonked;
        let mut binders = Vec::new();

        for evar in evars {
            let (universal_id, universal) = self.fresh_universal("t");
            let kind = ctx.evar_kind_id(evar).unwrap_or(Kind::Star);

            result = result.substitute_evar(evar, &TypeExpr::make_var_id("t", universal_id));
            let _ = universal;
            binders.push((universal_id, kind));
        }

        for (name, kind) in binders.into_iter().rev() {
            result = TypeExpr::make_forall_id(name, kind, result);
        }

        result
    }
}

#[cfg(test)]
mod entailment_tests {
    use super::*;
    use solveig_ast::ast::{InstanceId, ModuleId};

    const EQ: ClassId = ClassId(1);
    const ORD: ClassId = ClassId(2);
    const INT: TyConId = TyConId(10);
    const MAYBE: TyConId = TyConId(11);
    const LIST: TyConId = TyConId(12);
    const PAIR: TyConId = TyConId(13);

    fn tycon(id: TyConId) -> TypeExpr {
        TypeExpr::TyCon(id)
    }

    fn var(name: &str) -> TypeExpr {
        TypeExpr::TyVar(legacy_tyvar_id(name))
    }

    fn app(func: TypeExpr, arg: TypeExpr) -> TypeExpr {
        TypeExpr::make_app(func, arg)
    }

    fn maybe(arg: TypeExpr) -> TypeExpr {
        app(tycon(MAYBE), arg)
    }

    fn list(arg: TypeExpr) -> TypeExpr {
        app(tycon(LIST), arg)
    }

    fn pair(left: TypeExpr, right: TypeExpr) -> TypeExpr {
        app(app(tycon(PAIR), left), right)
    }

    fn pred(class_id: ClassId, arg: TypeExpr) -> Predicate {
        Predicate {
            class_id,
            args: vec![arg],
        }
    }

    fn instance(head: Predicate, context: Vec<Predicate>) -> InstanceDefinition<Span> {
        InstanceDefinition {
            def_module: ModuleId(0),
            context,
            head,
            methods: Vec::new(),
        }
    }

    fn tiny_checker() -> TypeChecker {
        let a = var("a");
        let b = var("b");
        let instance_defs = vec![
            instance(pred(EQ, tycon(INT)), Vec::new()),
            instance(pred(EQ, maybe(a.clone())), vec![pred(EQ, a.clone())]),
            instance(pred(EQ, list(a.clone())), vec![pred(EQ, a.clone())]),
            instance(
                pred(EQ, pair(a.clone(), b.clone())),
                vec![pred(EQ, a.clone()), pred(EQ, b.clone())],
            ),
        ];

        let env = TypeCheckEnv {
            constructor_types: HashMap::new(),
            func_schemes: HashMap::new(),
            class_defs: HashMap::from([
                (
                    EQ,
                    ClassDefinition {
                        id: EQ,
                        name: "test.Eq".to_string(),
                        short_name: "Eq".to_string(),
                        params: vec![("a".to_string(), Kind::Star)],
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
                        params: vec![("a".to_string(), Kind::Star)],
                        superclasses: vec![pred(EQ, var("a"))],
                        methods: Vec::new(),
                    },
                ),
            ]),
            instance_defs,
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            tycon_kinds: HashMap::new(),
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
                        vec![int.clone()],
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
            instance(pred(EQ, pair(var("a"), tycon(INT))), Vec::new()),
            instance(pred(EQ, pair(tycon(PRIM_TYS.bool), var("b"))), Vec::new()),
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
        let expected = TypeExpr::make_qualified(vec![pred(EQ, tycon(INT))], TypeExpr::TyUnit);
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
        let base = elab_node(TermExpr::Unit, 0..1, TypeExpr::TyUnit);
        let expr = wrap_ev_apps(base, destinations, 0..1, TypeExpr::TyUnit);

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
                ty: TypeExpr::TyUnit,
            },
        );
        let expr = TermExprNode::new(TermExpr::Var(VarRef::Global(def_id)), 0..1);

        let (_, elaborated, inferred_ty) = checker
            .solve_inference(Context::new(), &expr)
            .expect("global instantiation should infer");

        assert_eq!(inferred_ty, TypeExpr::TyUnit);
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
        let local_symbol = solveig_ast::ast::legacy_symbol_id("localQualified");
        let local_name = "localQualified".to_string();
        let expr = TermExprNode::new(TermExpr::Var(VarRef::Local(local_symbol)), 0..1);
        let qualified_type =
            TypeExpr::make_qualified(vec![pred(EQ, tycon(INT))], TypeExpr::TyUnit);
        let ctx = Context::new().add_entry(ContextEntry::TypedVar {
            var_name: local_name,
            ty: qualified_type,
        });

        let (_, elaborated) = checker
            .solve_checking(ctx, &expr, &TypeExpr::TyUnit)
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
