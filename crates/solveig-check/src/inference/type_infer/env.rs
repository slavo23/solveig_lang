type Span = Range<usize>;

#[derive(Clone, Debug)]
pub struct TcMetadata {
    pub span: Span,
    pub inferred_type: TypeExpr,
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
    pub fn new(span: Span, inferred_type: TypeExpr) -> Self {
        Self {
            span,
            inferred_type,
        }
    }
}

fn file_span(file: FileId, span: Span) -> FileSpan {
    FileSpan::new(file, SourceSpan::from_usize_range(span))
}

pub struct TypeChecker {
    file: FileId,
    names: Names,
    counter: u32,
    constructor_types: HashMap<CtorId, TypeExpr>,
    func_schemes: HashMap<DefId, Scheme>,
    class_defs: HashMap<ClassId, ClassDefinition>,
    instance_table: InstanceTable<Span>,
    tycon_table: TyConTable,
    ctor_table: CtorTable,
    class_table: ClassTable,
    tycon_kinds: HashMap<TyConId, Kind>,
    givens: Vec<GivenEvidence>,
    wanteds: Vec<Wanted>,
    evar_solutions: HashMap<EvarId, TypeExpr>,
    evidence_solutions: HashMap<EvVarId, Ev>,
    holes: Vec<HoleInfo>,
    warnings: Vec<TypeError>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            file: FileId(0),
            names: Names::new(),
            counter: 0,
            constructor_types: HashMap::new(),
            func_schemes: HashMap::new(),
            class_defs: HashMap::new(),
            instance_table: InstanceTable::new(),
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            tycon_kinds: HashMap::new(),
            givens: Vec::new(),
            wanteds: Vec::new(),
            evar_solutions: HashMap::new(),
            evidence_solutions: HashMap::new(),
            holes: Vec::new(),
            warnings: Vec::new(),
        }
    }
    pub fn with_env(env: &TypeCheckEnv, file: FileId) -> Self {
        Self {
            file,
            names: env.names.clone(),
            counter: 0,
            constructor_types: env.constructor_types.clone(),
            func_schemes: env.func_schemes.clone(),
            class_defs: env.class_defs.clone(),
            instance_table: InstanceTable::from_definitions(env.instance_defs.clone()),
            tycon_table: env.tycon_table.clone(),
            ctor_table: env.ctor_table.clone(),
            class_table: env.class_table.clone(),
            tycon_kinds: env.tycon_kinds.clone(),
            givens: Vec::new(),
            wanteds: Vec::new(),
            evar_solutions: HashMap::new(),
            evidence_solutions: HashMap::new(),
            holes: Vec::new(),
            warnings: Vec::new(),
        }
    }
    pub fn with_module_context(module: &ResolvedModule<Span>, file: FileId, names: &Names) -> Self {
        let env = TypeCheckEnv::from_module(module, names);
        Self::with_env(&env, file)
    }
    fn get_file_span(&self, span: Span) -> FileSpan {
        file_span(self.file, span)
    }
    fn pretty_tables(&self) -> PrettyTables<'_> {
        PrettyTables {
            names: &self.names,
            tycons: &self.tycon_table,
            classes: &self.class_table,
        }
    }
    fn pretty_type(&self, ty: &TypeExpr) -> String {
        ty.pretty_with_tables(self.pretty_tables())
    }
    fn pretty_predicate(&self, pred: &Predicate) -> String {
        pred.pretty_with_tables(self.pretty_tables())
    }
    fn pretty_scheme(&self, scheme: &Scheme) -> String {
        scheme.pretty_with_tables(self.pretty_tables())
    }
    fn display_class_name(&self, id: ClassId) -> String {
        self.class_table
            .get(id)
            .map(|info| self.names.symbol_display_name(info.name))
            .unwrap_or_else(|| panic!("ICE: type checker class table missing ClassId {id:?}"))
    }
    fn display_ctor_name(&self, id: CtorId) -> String {
        self.ctor_table
            .get(id)
            .map(|info| self.names.symbol_display_name(info.name))
            .unwrap_or_else(|| panic!("ICE: type checker constructor table missing CtorId {id:?}"))
    }
    fn fresh_id(&mut self) -> u32 {
        self.counter += 1;
        self.counter
    }
    fn fresh_evar_id(&mut self) -> EvarId {
        EvarId(self.fresh_id())
    }
    fn fresh_tyvar_id(&mut self) -> TyVarId {
        TyVarId(self.fresh_id())
    }
    fn fresh_evvar_id(&mut self) -> EvVarId {
        EvVarId(self.fresh_id())
    }
    fn evar_type(id: EvarId) -> TypeExpr {
        TypeExpr::make_evar(id)
    }
    fn fresh_universal(&mut self, base: &str) -> (TyVarId, String) {
        let id = self.fresh_tyvar_id();
        (id, generated_tyvar_name(base, id))
    }
    fn universal_entry(id: TyVarId, kind: Kind) -> ContextEntry {
        ContextEntry::UniVar {
            id,
            name: tyvar_display_name(id),
            kind,
        }
    }
    fn scheme_to_type(scheme: &Scheme) -> TypeExpr {
        scheme.to_type_expr()
    }
    fn type_to_scheme(ty: &TypeExpr) -> Scheme {
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
        let mut quantifiers = class_def
            .params
            .iter()
            .map(|(name, kind)| (source_tyvar_id(name), kind.clone()))
            .collect::<Vec<_>>();

        for (id, kind) in &method.scheme.quantifiers {
            if !quantifiers.iter().any(|(existing, _)| existing == id) {
                quantifiers.push((*id, kind.clone()));
            }
        }

        Scheme {
            quantifiers,
            predicates: method.scheme.predicates.clone(),
            ty: method.scheme.ty.clone(),
        }
    }
    fn lookup_class_method_scheme(&self, id: DefId) -> Option<Scheme> {
        self.class_defs.values().find_map(|class_def| {
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
}

pub type CheckedModule = ResolvedModule<TcMetadata, Span>;

#[derive(Clone, Debug)]
pub struct TypeCheckEnv {
    pub names: Names,
    pub constructor_types: HashMap<CtorId, TypeExpr>,
    pub func_schemes: HashMap<DefId, Scheme>,
    pub class_defs: HashMap<ClassId, ClassDefinition>,
    pub instance_defs: Vec<InstanceDefinition<Span>>,
    pub tycon_table: TyConTable,
    pub ctor_table: CtorTable,
    pub class_table: ClassTable,
    pub tycon_kinds: HashMap<TyConId, Kind>,
}

impl TypeCheckEnv {
    pub fn from_module(module: &ResolvedModule<Span>, names: &Names) -> Self {
        Self {
            names: names.clone(),
            constructor_types: module.constructor_types.clone(),
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

    pub fn from_checked_module(module: &CheckedModule, names: &Names) -> Self {
        let mut expr_anno = |metadata: TcMetadata| metadata.span;
        let mut pattern_anno = |span: Span| span;

        Self {
            names: names.clone(),
            constructor_types: module.constructor_types.clone(),
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

    pub fn pretty_tables(&self) -> PrettyTables<'_> {
        PrettyTables {
            names: &self.names,
            tycons: &self.tycon_table,
            classes: &self.class_table,
        }
    }

    fn display_class_name(&self, id: ClassId) -> String {
        self.class_table
            .get(id)
            .map(|info| self.names.symbol_display_name(info.name))
            .unwrap_or_else(|| panic!("ICE: type check env class table missing ClassId {id:?}"))
    }
}

pub struct TCOutput {
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

        let mut env = TypeCheckEnv {
            names: Names::new(),
            constructor_types: HashMap::new(),
            func_schemes: HashMap::new(),
            class_defs: HashMap::new(),
            instance_defs: Vec::new(),
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            tycon_kinds: HashMap::new(),
        };
        env.func_schemes
            .insert(def_id, Scheme::mono(TypeExpr::TyUnit));

        let mut checker = TypeChecker::with_env(&env, FileId(0));
        let expr = TermExprNode::new(TermExpr::Var(VarRef::Global(def_id)), 0..0);

        let result = checker.solve_inference(Context::new(), &expr);
        assert!(matches!(result, Ok((_, _, TypeExpr::TyUnit))));

        let missing_expr = TermExprNode::new(TermExpr::Var(VarRef::Global(other_def_id)), 0..0);
        assert!(
            checker
                .solve_inference(Context::new(), &missing_expr)
                .is_err()
        );
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
            names,
            constructor_types: HashMap::new(),
            func_schemes: HashMap::new(),
            class_defs: HashMap::from([(
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
                        scheme: Scheme::mono(TypeExpr::TyUnit),
                        default_body: None,
                    }],
                },
            )]),
            instance_defs: Vec::new(),
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            tycon_kinds: HashMap::new(),
        };

        let mut checker = TypeChecker::with_env(&env, FileId(0));
        let expr = TermExprNode::new(TermExpr::Var(VarRef::Global(method_id)), 0..0);

        let result = checker.solve_inference(Context::new(), &expr);
        assert!(matches!(result, Ok((_, _, TypeExpr::TyUnit))));

        let missing_expr =
            TermExprNode::new(TermExpr::Var(VarRef::Global(other_method_id)), 0..0);
        assert!(
            checker
                .solve_inference(Context::new(), &missing_expr)
                .is_err()
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
            tycon_kinds: HashMap::new(),
            constructor_types: HashMap::new(),
            func_schemes: HashMap::new(),
            func_defs: Vec::new(),
        };

        let env = TypeCheckEnv::from_module(&module, &names);

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
        let int_ty = TypeExpr::make_named_con(&mut names, "test.Int".to_string());

        let class_id = names.class_id(&class_name);
        let superclass_id = names.class_id(&superclass_name);
        let env = TypeCheckEnv {
            names: names.clone(),
            constructor_types: HashMap::new(),
            func_schemes: HashMap::new(),
            class_defs: HashMap::from([(
                class_id,
                ClassDefinition {
                    id: class_id,
                    name: class_name.clone(),
                    short_name: "Sub".to_string(),
                    params: vec![("a".to_string(), Kind::Star)],
                    superclasses: vec![Predicate::named(
                        &mut names,
                        superclass_name.clone(),
                        vec![TypeExpr::make_var("a".to_string())],
                    )],
                    methods: Vec::new(),
                },
            )]),
            instance_defs: Vec::new(),
            tycon_table: TyConTable::new(),
            ctor_table: CtorTable::new(),
            class_table: ClassTable::new(),
            tycon_kinds: HashMap::new(),
        };

        let checker = TypeChecker::with_env(&env, FileId(0));
        let given = Predicate::named(&mut names, class_name, vec![int_ty.clone()]);
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
            names,
            constructor_types: HashMap::new(),
            func_schemes: HashMap::new(),
            class_defs: HashMap::new(),
            instance_defs: Vec::new(),
            tycon_table,
            ctor_table: CtorTable::new(),
            class_table,
            tycon_kinds: HashMap::new(),
        };

        let checker = TypeChecker::with_env(&env, FileId(0));
        let widget = TypeExpr::TyCon(tycon_id);
        let pred = Predicate {
            class_id,
            args: vec![widget.clone()],
        };

        assert_eq!(checker.pretty_type(&widget), "Widget");
        assert_eq!(checker.pretty_predicate(&pred), "Showy Widget");
    }
}
