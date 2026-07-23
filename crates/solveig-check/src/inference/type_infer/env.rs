type Span = Range<usize>;

#[derive(Clone, Debug)]
pub struct TcMetadata {
    pub span: Span,
    pub inferred_type: TypeExpr,
}

type TcResult<T> = Result<T, TypeError>;

#[derive(Clone, Debug)]
struct Wanted {
    pred: Predicate,
    span: Span,
    givens: Vec<Predicate>,
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
    counter: u32,
    constructor_types: HashMap<String, TypeExpr>,
    func_schemes: HashMap<String, Scheme>,
    class_defs: Vec<ClassDefinition>,
    instance_defs: Vec<InstanceDefinition<Span>>,
    tycon_kinds: HashMap<String, Kind>,
    givens: Vec<Predicate>,
    wanteds: Vec<Wanted>,
    evar_solutions: HashMap<EvarId, TypeExpr>,
    holes: Vec<HoleInfo>,
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
            counter: 0,
            constructor_types: HashMap::new(),
            func_schemes: HashMap::new(),
            class_defs: Vec::new(),
            instance_defs: Vec::new(),
            tycon_kinds: HashMap::new(),
            givens: Vec::new(),
            wanteds: Vec::new(),
            evar_solutions: HashMap::new(),
            holes: Vec::new(),
        }
    }
    pub fn with_env(env: &TypeCheckEnv, file: FileId) -> Self {
        Self {
            file,
            counter: 0,
            constructor_types: env.constructor_types.clone(),
            func_schemes: env.func_schemes.clone(),
            class_defs: env.class_defs.clone(),
            instance_defs: env.instance_defs.clone(),
            tycon_kinds: env.tycon_kinds.clone(),
            givens: Vec::new(),
            wanteds: Vec::new(),
            evar_solutions: HashMap::new(),
            holes: Vec::new(),
        }
    }
    pub fn with_module_context(module: &ResolvedModule<Span>, file: FileId) -> Self {
        let env = TypeCheckEnv::from_module(module);
        Self::with_env(&env, file)
    }
    fn get_file_span(&self, span: Span) -> FileSpan {
        file_span(self.file, span)
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
    fn evar_type(id: EvarId) -> TypeExpr {
        TypeExpr::make_evar(id)
    }
    fn fresh_universal(&mut self, base: &str) -> (TyVarId, String) {
        let id = self.fresh_tyvar_id();
        (id, legacy_tyvar_name(base, id))
    }
    fn fresh_universal_entry(&mut self, name: String, kind: Kind) -> ContextEntry {
        ContextEntry::UniVar {
            id: self.fresh_tyvar_id(),
            name,
            kind,
        }
    }
    fn scheme_to_type(scheme: &Scheme) -> TypeExpr {
        let mut ty = scheme.ty.clone();

        if !scheme.predicates.is_empty() {
            ty = TypeExpr::make_qualified(scheme.predicates.clone(), ty);
        }

        for (name, kind) in scheme.quantifiers.iter().rev() {
            ty = TypeExpr::make_forall(name.clone(), kind.clone(), ty);
        }

        ty
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
        let mut quantifiers = class_def.params.clone();

        for (name, kind) in &method.scheme.quantifiers {
            if !quantifiers.iter().any(|(existing, _)| existing == name) {
                quantifiers.push((name.clone(), kind.clone()));
            }
        }

        Scheme {
            quantifiers,
            predicates: method.scheme.predicates.clone(),
            ty: method.scheme.ty.clone(),
        }
    }
    fn lookup_class_method_scheme(&self, name: &str) -> Option<Scheme> {
        self.class_defs.iter().find_map(|class_def| {
            class_def.methods.iter().find_map(|method| {
                let short_name = method.name.rsplit('.').next().unwrap_or(&method.name);

                if method.name == name || short_name == name {
                    Some(Self::class_method_scheme_for_term_lookup(class_def, method))
                } else {
                    None
                }
            })
        })
    }
}

pub type CheckedModule = ResolvedModule<TcMetadata, Span>;

#[derive(Clone, Debug)]
pub struct TypeCheckEnv {
    pub constructor_types: HashMap<String, TypeExpr>,
    pub func_schemes: HashMap<String, Scheme>,
    pub class_defs: Vec<ClassDefinition>,
    pub instance_defs: Vec<InstanceDefinition<Span>>,
    pub tycon_kinds: HashMap<String, Kind>,
}

impl TypeCheckEnv {
    pub fn from_module(module: &ResolvedModule<Span>) -> Self {
        Self {
            constructor_types: module.constructor_types.clone(),
            func_schemes: module.func_schemes.clone(),
            class_defs: module.class_defs.clone(),
            instance_defs: module.instance_defs.clone(),
            tycon_kinds: module.tycon_kinds.clone(),
        }
    }
}

pub struct TCOutput {
    pub module: CheckedModule,
    pub holes: Vec<HoleInfo>,
}
