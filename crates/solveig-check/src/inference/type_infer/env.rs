type Span = Range<usize>;

#[derive(Clone, Debug)]
pub struct TcMetadata {
    pub span: Span,
    pub inferred_type: Ty,
}

type TcResult<T> = Result<T, TypeError>;

#[derive(Clone, Debug)]
struct GivenEvidence {
    ev: EvVarId,
    pred: Predicate,
}

#[derive(Clone, Debug)]
struct Wanted {
    ev: EvVarId,
    pred: Predicate,
    span: Span,
    givens: Vec<GivenEvidence>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Unsolved {
    pub pred: Predicate,
}

impl TcMetadata {
    pub fn new(span: Span, inferred_type: Ty) -> Self {
        Self {
            span,
            inferred_type,
        }
    }
}

fn file_span(file: FileId, span: Span) -> FileSpan {
    FileSpan::new(file, SourceSpan::from_usize_range(span))
}

pub struct TypeChecker<'a> {
    file: FileId,
    names: &'a Names,
    tables: ModuleTables<'a>,
    delta: CheckDelta,
    counter: u32,
    disabled_instance: Option<InstanceId>,
    givens: Vec<GivenEvidence>,
    wanteds: Vec<Wanted>,
    evar_solutions: FxHashMap<EvarId, Ty>,
    evidence_solutions: FxHashMap<EvVarId, Ev>,
    holes: Vec<HoleInfo>,
    warnings: Vec<TypeError>,
}

#[derive(Clone)]
struct TypeCheckerCheckpoint {
    names: usize,
    counter: u32,
    givens: Vec<GivenEvidence>,
    wanteds: Vec<Wanted>,
    evar_solutions: FxHashMap<EvarId, Ty>,
    evidence_solutions: FxHashMap<EvVarId, Ev>,
    holes: Vec<HoleInfo>,
    warnings: Vec<TypeError>,
}

impl<'a> TypeChecker<'a> {
    pub fn with_env(env: &'a TypeCheckEnv, names: &'a Names, file: FileId) -> Self {
        Self::with_delta(
            ModuleTables::from_env(env),
            names,
            file,
            CheckDelta::from_env(env, names),
        )
    }

    fn with_delta(
        tables: ModuleTables<'a>,
        names: &'a Names,
        file: FileId,
        delta: CheckDelta,
    ) -> Self {
        Self {
            file,
            names,
            tables,
            delta,
            counter: 0,
            disabled_instance: None,
            givens: Vec::new(),
            wanteds: Vec::new(),
            evar_solutions: FxHashMap::default(),
            evidence_solutions: FxHashMap::default(),
            holes: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn finish(self) -> (CheckDelta, Vec<HoleInfo>, Vec<TypeError>) {
        (self.delta, self.holes, self.warnings)
    }

    fn get_file_span(&self, span: Span) -> FileSpan {
        file_span(self.file, span)
    }
    fn pretty_tables(&self) -> PrettyTables<'_> {
        PrettyTables::new(self.names, self.tables.tycon_table, self.tables.class_table)
            .with_name_scratch(&self.delta.names)
    }
    fn pretty_type(&self, ty: &Ty) -> String {
        ty.pretty_with_tables(self.pretty_tables())
    }
    fn pretty_predicate(&self, pred: &Predicate) -> String {
        pred.pretty_with_tables(self.pretty_tables())
    }
    fn pretty_scheme(&self, scheme: &Scheme) -> String {
        scheme.pretty_with_tables(self.pretty_tables())
    }
    fn display_ctor_name(&self, id: CtorId) -> String {
        self.tables.ctor_table.get(id).map_or_else(
            || solveig_base::ice!("type checker constructor table missing CtorId {id:?}"),
            |info| self.names.symbol_display_name(info.name),
        )
    }
    fn fresh_id(&mut self) -> u32 {
        self.counter += 1;
        self.counter
    }
    fn fresh_evar_id(&mut self) -> EvarId {
        EvarId(self.fresh_id())
    }
    fn fresh_evvar_id(&mut self) -> EvVarId {
        EvVarId(self.fresh_id())
    }
    fn evar_type(id: EvarId) -> Ty {
        Ty::make_evar(id)
    }
    fn fresh_universal(&mut self, base: &str) -> (TyVarId, String) {
        self.delta.names.fresh_generated_tyvar(base)
    }
    fn tyvar_display_name(&self, id: TyVarId) -> String {
        self.delta.names.tyvar_display_name(self.names, id)
    }
    fn universal_entry(&self, id: TyVarId, kind: Kind) -> ContextEntry {
        ContextEntry::UniVar {
            id,
            name: self.tyvar_display_name(id),
            kind,
        }
    }
    fn scheme_to_type(scheme: &Scheme) -> Ty {
        scheme.to_ty()
    }
    fn type_to_scheme(ty: &Ty) -> Scheme {
        let (quantifiers, predicates, body) = ty.peel_scheme_like();
        Scheme {
            quantifiers,
            predicates,
            ty: body,
        }
    }
    fn class_method_scheme_for_term_lookup(
        class_def: &ClassDefinition,
        method: &ClassMethodDef,
    ) -> Scheme {
        let mut quantifiers = class_def.params.clone();
        quantifiers.extend(method.scheme.quantifiers.clone());

        Scheme {
            quantifiers,
            predicates: method.scheme.predicates.clone(),
            ty: method.scheme.ty.clone(),
        }
    }
    fn lookup_class_method_scheme(&self, id: DefId) -> Option<Scheme> {
        self.tables.class_defs.values().find_map(|class_def| {
            class_def.methods.iter().find_map(|method| {
                if method.id == id {
                    Some(Self::class_method_scheme_for_term_lookup(class_def, method))
                } else {
                    None
                }
            })
        })
    }
    pub fn evidence_solution(&self, ev: EvVarId) -> Option<&Ev> {
        self.evidence_solutions.get(&ev)
    }
    fn checkpoint(&self) -> TypeCheckerCheckpoint {
        TypeCheckerCheckpoint {
            names: self.delta.names.checkpoint(),
            counter: self.counter,
            givens: self.givens.clone(),
            wanteds: self.wanteds.clone(),
            evar_solutions: self.evar_solutions.clone(),
            evidence_solutions: self.evidence_solutions.clone(),
            holes: self.holes.clone(),
            warnings: self.warnings.clone(),
        }
    }
    fn restore(&mut self, checkpoint: TypeCheckerCheckpoint) {
        self.delta.names.restore(checkpoint.names);
        self.counter = checkpoint.counter;
        self.givens = checkpoint.givens;
        self.wanteds = checkpoint.wanteds;
        self.evar_solutions = checkpoint.evar_solutions;
        self.evidence_solutions = checkpoint.evidence_solutions;
        self.holes = checkpoint.holes;
        self.warnings = checkpoint.warnings;
    }
}

pub type CheckedModule = ResolvedModule<TcMetadata, Span>;

/// Owns the resolved APIs available to a module before type checking.
#[derive(Debug)]
pub struct TypeCheckEnv {
    pub constructor_types: FxHashMap<CtorId, Ty>,
    pub hole_constructor_ids: Vec<CtorId>,
    pub func_schemes: FxHashMap<DefId, Scheme>,
    pub class_defs: FxHashMap<ClassId, ClassDefinition>,
    pub instance_defs: Vec<InstanceDefinition<Span>>,
    pub tycon_table: TyConTable,
    pub ctor_table: CtorTable,
    pub class_table: ClassTable,
    pub tycon_kinds: FxHashMap<TyConId, Kind>,
}

/// Immutable type-checking tables borrowed for one module check.
#[derive(Clone, Copy)]
pub struct ModuleTables<'a> {
    pub constructor_types: &'a FxHashMap<CtorId, Ty>,
    pub hole_constructor_ids: &'a [CtorId],
    pub class_defs: &'a FxHashMap<ClassId, ClassDefinition>,
    pub instance_defs: &'a [InstanceDefinition<Span>],
    pub tycon_table: &'a TyConTable,
    pub ctor_table: &'a CtorTable,
    pub class_table: &'a ClassTable,
    pub tycon_kinds: &'a FxHashMap<TyConId, Kind>,
}

/// Contains the mutable state accumulated during one module check.
#[derive(Clone, Debug)]
pub struct CheckDelta {
    pub func_schemes: FxHashMap<DefId, Scheme>,
    pub names: NameScratch,
}

impl TypeCheckEnv {
    pub fn from_module(module: &ResolvedModule<Span>) -> Self {
        Self {
            constructor_types: module.constructor_types.clone(),
            hole_constructor_ids: sorted_constructor_ids(&module.constructor_types),
            func_schemes: module.func_schemes.clone(),
            class_defs: module
                .class_defs
                .iter()
                .map(|class_def| (class_def.id, class_def.clone()))
                .collect(),
            instance_defs: module.instance_defs.clone(),
            tycon_table: module.tycon_table.clone(),
            ctor_table: module.ctor_table.clone(),
            class_table: module.class_table.clone(),
            tycon_kinds: module.tycon_kinds.clone(),
        }
    }

    pub fn from_checked_module(module: &CheckedModule) -> Self {
        let mut expr_anno = |metadata: TcMetadata| metadata.span;
        let mut pattern_anno = |span: Span| span;

        Self {
            constructor_types: module.constructor_types.clone(),
            hole_constructor_ids: sorted_constructor_ids(&module.constructor_types),
            func_schemes: module.func_schemes.clone(),
            class_defs: module
                .class_defs
                .iter()
                .map(|class_def| (class_def.id, class_def.clone()))
                .collect(),
            instance_defs: module
                .instance_defs
                .clone()
                .into_iter()
                .map(|inst| inst.map_anno(&mut expr_anno, &mut pattern_anno))
                .collect(),
            tycon_table: module.tycon_table.clone(),
            ctor_table: module.ctor_table.clone(),
            class_table: module.class_table.clone(),
            tycon_kinds: module.tycon_kinds.clone(),
        }
    }

    pub fn pretty_tables<'a>(&'a self, names: &'a Names) -> PrettyTables<'a> {
        PrettyTables::new(names, &self.tycon_table, &self.class_table)
    }

    fn display_class_name(&self, names: &Names, id: ClassId) -> String {
        self.class_table.get(id).map_or_else(
            || solveig_base::ice!("type check env class table missing ClassId {id:?}"),
            |info| names.symbol_display_name(info.name),
        )
    }
}

impl<'a> ModuleTables<'a> {
    fn from_env(env: &'a TypeCheckEnv) -> Self {
        Self {
            constructor_types: &env.constructor_types,
            hole_constructor_ids: &env.hole_constructor_ids,
            class_defs: &env.class_defs,
            instance_defs: &env.instance_defs,
            tycon_table: &env.tycon_table,
            ctor_table: &env.ctor_table,
            class_table: &env.class_table,
            tycon_kinds: &env.tycon_kinds,
        }
    }
}

fn sorted_constructor_ids(constructors: &FxHashMap<CtorId, Ty>) -> Vec<CtorId> {
    let mut ids = constructors.keys().copied().collect::<Vec<_>>();
    ids.sort();
    ids
}

impl CheckDelta {
    fn from_env(env: &TypeCheckEnv, names: &Names) -> Self {
        Self {
            func_schemes: env.func_schemes.clone(),
            names: NameScratch::from_names(names),
        }
    }
}

pub struct TCOutput {
    pub name_scratch: NameScratch,
    pub module: CheckedModule,
    pub holes: Vec<HoleInfo>,
    pub diagnostics: Vec<TypeError>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use solveig_ast::ast::{ClassInfo, Names, TyConDefinition, TyConInfo};

    #[test]
    fn global_function_schemes_are_keyed_by_defid() {
        let def_id = DefId {
            module: solveig_ast::ast::ModuleId(2),
            name: solveig_base::Symbol(3),
        };
        let other_def_id = DefId {
            module: solveig_ast::ast::ModuleId(2),
            name: solveig_base::Symbol(4),
        };

        let names = Names::new();
        let mut env = TypeCheckEnv {
            constructor_types: FxHashMap::default(),
            hole_constructor_ids: Vec::new(),
            func_schemes: FxHashMap::default(),
            class_defs: FxHashMap::default(),
            instance_defs: Vec::new(),
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            tycon_kinds: FxHashMap::default(),
        };
        env.func_schemes
            .insert(def_id, Scheme::mono(Ty::TyUnit));

        let mut checker = TypeChecker::with_env(&env, &names, FileId(0));
        let expr = TermExprNode::new(TermExpr::Var(VarRef::Global(def_id)), 0..0);

        let result = checker.solve_inference(Context::new(), &expr);
        assert!(matches!(result, Ok((_, _, Ty::TyUnit))));

        let missing_expr = TermExprNode::new(TermExpr::Var(VarRef::Global(other_def_id)), 0..0);
        checker
            .solve_inference(Context::new(), &missing_expr)
            .unwrap_err();
    }

    #[test]
    fn class_method_schemes_are_keyed_by_defid() {
        let method_id = DefId {
            module: solveig_ast::ast::ModuleId(5),
            name: solveig_base::Symbol(8),
        };
        let other_method_id = DefId {
            module: solveig_ast::ast::ModuleId(5),
            name: solveig_base::Symbol(9),
        };

        let mut names = Names::new();
        let class_id = names.class_id("C");
        let env = TypeCheckEnv {
            constructor_types: FxHashMap::default(),
            hole_constructor_ids: Vec::new(),
            func_schemes: FxHashMap::default(),
            class_defs: FxHashMap::from_iter([(
                class_id,
                ClassDefinition {
                    id: class_id,
                    name: "C".to_string(),
                    short_name: "C".to_string(),
                    params: Vec::new(),
                    superclasses: Vec::new(),
                    methods: vec![ClassMethodDef {
                        id: method_id,
                        name: "C.method".to_string(),
                        short_name: "method".to_string(),
                        scheme: Scheme::mono(Ty::TyUnit),
                        default_body: None,
                    }],
                },
            )]),
            instance_defs: Vec::new(),
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            tycon_kinds: FxHashMap::default(),
        };

        let mut checker = TypeChecker::with_env(&env, &names, FileId(0));
        let expr = TermExprNode::new(TermExpr::Var(VarRef::Global(method_id)), 0..0);

        let result = checker.solve_inference(Context::new(), &expr);
        assert!(matches!(result, Ok((_, _, Ty::TyUnit))));

        let missing_expr =
            TermExprNode::new(TermExpr::Var(VarRef::Global(other_method_id)), 0..0);
        checker
            .solve_inference(Context::new(), &missing_expr)
            .unwrap_err();
    }

    #[test]
    fn module_check_returns_name_scratch_without_mutating_names() {
        let file = FileId(0);
        let mut names = Names::new();
        let module_id = solveig_ast::ast::ModuleId(3);
        let def_id = DefId {
            module: module_id,
            name: names.symbol("test.identity"),
        };
        names.register_def(def_id, "test.identity");
        let param_symbol = names.symbol("x");
        let next_before = names.next_tyvar_id();
        let body = TermExprNode::new(
            TermExpr::Abs(
                "x".to_string(),
                None,
                Box::new(TermExprNode::new(
                    TermExpr::Var(VarRef::Local(param_symbol)),
                    2..3,
                )),
            ),
            0..3,
        );
        let module = ResolvedModule::<Span> {
            data_defs: Vec::new(),
            class_defs: Vec::new(),
            instance_defs: Vec::new(),
            builtin_defs: Vec::new(),
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            tycon_kinds: FxHashMap::default(),
            constructor_types: FxHashMap::default(),
            func_schemes: FxHashMap::default(),
            func_defs: vec![TermDefinition {
                id: def_id,
                name: "test.identity".to_string(),
                short_name: "identity".to_string(),
                signature: None,
                body,
            }],
        };
        let env = TypeCheckEnv::from_module(&module);

        let output = check_and_elab_module(&module, &env, &names, file)
            .expect("unsignatured identity should check");

        assert_eq!(names.next_tyvar_id(), next_before);
        let scheme = output
            .module
            .func_schemes
            .get(&def_id)
            .expect("checked module should publish inferred scheme");
        let (quantifier, _) = scheme
            .quantifiers
            .first()
            .cloned()
            .expect("identity should infer one quantifier");
        assert_eq!(
            output.name_scratch.tyvar_display_name(&names, quantifier),
            "t0"
        );

        output.name_scratch.merge_into(&mut names);
        assert_eq!(names.next_tyvar_id(), TyVarId(1));
        assert_eq!(names.tyvar_display_name(quantifier), "t0");
    }

    #[test]
    fn class_method_lookup_keeps_shadowed_type_variables_distinct() {
        let mut names = Names::new();
        let class_id = names.class_id("C");
        let class_param = names.fresh_tyvar("a");
        let method_param = names.fresh_tyvar("a");
        let method_id = DefId {
            module: solveig_ast::ast::ModuleId(5),
            name: solveig_base::Symbol(8),
        };
        let method_scheme = Scheme {
            quantifiers: vec![(method_param, Kind::Star)],
            predicates: vec![Predicate {
                class_id,
                args: vec![Ty::TyVar(class_param)],
            }],
            ty: Ty::make_arr(Ty::TyVar(method_param), Ty::TyVar(method_param)),
        };
        let class_def = ClassDefinition {
            id: class_id,
            name: "C".to_string(),
            short_name: "C".to_string(),
            params: vec![(class_param, Kind::Star)],
            superclasses: Vec::new(),
            methods: Vec::new(),
        };
        let method = ClassMethodDef {
            id: method_id,
            name: "C.method".to_string(),
            short_name: "method".to_string(),
            scheme: method_scheme,
            default_body: None,
        };

        let lookup_scheme = TypeChecker::class_method_scheme_for_term_lookup(&class_def, &method);

        assert_ne!(class_param, method_param);
        assert_eq!(
            lookup_scheme.quantifiers,
            vec![(class_param, Kind::Star), (method_param, Kind::Star)]
        );
    }

    #[test]
    fn typecheck_env_class_defs_are_keyed_by_classid() {
        let mut names = Names::new();
        let class_id = names.class_id("test.EnvClass");
        let module = ResolvedModule::<Span> {
            data_defs: Vec::new(),
            class_defs: vec![ClassDefinition {
                id: class_id,
                name: "test.EnvClass".to_string(),
                short_name: "EnvClass".to_string(),
                params: Vec::new(),
                superclasses: Vec::new(),
                methods: Vec::new(),
            }],
            instance_defs: Vec::new(),
            builtin_defs: Vec::new(),
            tycon_table: Default::default(),
            ctor_table: Default::default(),
            class_table: Default::default(),
            tycon_kinds: FxHashMap::default(),
            constructor_types: FxHashMap::default(),
            func_schemes: FxHashMap::default(),
            func_defs: Vec::new(),
        };

        let env = TypeCheckEnv::from_module(&module);

        assert!(env.class_defs.contains_key(&class_id));
        assert!(!env
            .class_defs
            .contains_key(&names.class_id("test.OtherEnvClass")));
    }

    #[test]
    fn superclass_lookup_uses_predicate_class_ids() {
        let superclass_name = "test.Sup".to_string();
        let class_name = "test.Sub".to_string();
        let mut names = Names::new();
        let int_ty = Ty::make_named_con(&mut names, "test.Int");

        let class_id = names.class_id(&class_name);
        let superclass_id = names.class_id(&superclass_name);
        let param = names.fresh_tyvar("a");
        let superclass = Predicate::named(&mut names, &superclass_name, vec![Ty::TyVar(param)]);
        let env = TypeCheckEnv {
            constructor_types: FxHashMap::default(),
            hole_constructor_ids: Vec::new(),
            func_schemes: FxHashMap::default(),
            class_defs: FxHashMap::from_iter([(
                class_id,
                ClassDefinition {
                    id: class_id,
                    name: class_name.clone(),
                    short_name: "Sub".to_string(),
                    params: vec![(param, Kind::Star)],
                    superclasses: vec![superclass],
                    methods: Vec::new(),
                },
            )]),
            instance_defs: Vec::new(),
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            tycon_kinds: FxHashMap::default(),
        };

        let given = Predicate::named(&mut names, &class_name, vec![int_ty.clone()]);
        let checker = TypeChecker::with_env(&env, &names, FileId(0));
        let superclasses = checker.superclass_predicates_for_given(&given);

        assert_eq!(superclasses.len(), 1);
        assert_eq!(
            superclasses[0].class_id,
            superclass_id
        );
        assert_eq!(superclasses[0].args, vec![int_ty]);
    }

    #[test]
    fn typechecker_pretty_uses_env_tables() {
        let tycon_id = TyConId(88);
        let class_id = ClassId(77);
        let mut names = Names::new();

        let mut tycon_table = TyConTable::new();
        tycon_table.insert_with_id(
            tycon_id,
            TyConInfo {
                name: names.symbol("test.pretty.Widget"),
                short_name: names.symbol("Widget"),
                kind: Kind::Star,
                definition: TyConDefinition::Primitive,
            },
        );

        let mut class_table = ClassTable::new();
        class_table.insert_with_id(
            class_id,
            ClassInfo {
                name: names.symbol("test.pretty.Showy"),
                short_name: names.symbol("Showy"),
                definition: ClassDefinition {
                    id: class_id,
                    name: "test.pretty.Showy".to_string(),
                    short_name: "Showy".to_string(),
                    params: Vec::new(),
                    superclasses: Vec::new(),
                    methods: Vec::new(),
                },
            },
        );

        let env = TypeCheckEnv {
            constructor_types: FxHashMap::default(),
            hole_constructor_ids: Vec::new(),
            func_schemes: FxHashMap::default(),
            class_defs: FxHashMap::default(),
            instance_defs: Vec::new(),
            tycon_table,
            ctor_table: CtorTable::new(),
            class_table,
            tycon_kinds: FxHashMap::default(),
        };

        let checker = TypeChecker::with_env(&env, &names, FileId(0));
        let widget = Ty::TyCon(tycon_id);
        let pred = Predicate {
            class_id,
            args: vec![widget.clone()],
        };

        assert_eq!(checker.pretty_type(&widget), "Widget");
        assert_eq!(checker.pretty_predicate(&pred), "Showy Widget");
    }
}
