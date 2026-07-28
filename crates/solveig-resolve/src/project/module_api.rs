//! Public and typed module API structures.
//! Resolution and later compiler stages use these maps to connect definitions across modules.

use super::*;

#[derive(Debug, Clone)]
pub struct ModuleTable {
    names: Vec<ModuleName>,
    ids: FxHashMap<ModuleName, ModuleId>,
}

impl ModuleTable {
    pub fn new<'a>(session: &Session, modules: impl IntoIterator<Item = &'a ModuleName>) -> Self {
        let mut names = modules.into_iter().cloned().collect::<Vec<_>>();
        names.sort_by_key(|module| module.as_string(session));
        names.dedup();

        let ids = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let id = ModuleId(
                    u32::try_from(index).expect("ICE: module table exceeded u32 capacity"),
                );
                (name.clone(), id)
            })
            .collect();

        Self { names, ids }
    }

    pub fn id(&self, module: &ModuleName) -> Option<ModuleId> {
        self.ids.get(module).copied()
    }

    pub fn name(&self, id: ModuleId) -> &ModuleName {
        self.names
            .get(id.0 as usize)
            .expect("ICE: ModuleId not present in ModuleTable")
    }

    pub fn def_path(&self, def: DefId) -> DefPath<'_> {
        DefPath {
            module: self.name(def.module),
            name: def.name,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DefPath<'a> {
    module: &'a ModuleName,
    name: Symbol,
}

impl DefPath<'_> {
    pub fn render(self, session: &Session) -> String {
        if self.module.0.is_empty() {
            return session.str(self.name).to_string();
        }

        let mut rendered = self.module.as_string(session);
        rendered.push('.');
        rendered.push_str(&session.str(self.name));
        rendered
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveNameKind {
    Term,
    Operator,
    TypeConstructor,
    Constructor,
    Class,
    Signature,
    Fixity,
}

impl ResolveNameKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Term => "term",
            Self::Operator => "operator",
            Self::TypeConstructor => "type constructor",
            Self::Constructor => "constructor",
            Self::Class => "class",
            Self::Signature => "signature",
            Self::Fixity => "fixity",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ResolveError {
    AmbiguousImport {
        span: FileSpan,
        kind: ResolveNameKind,
        name: Symbol,
        candidates: Vec<DefId>,
    },
    UnknownModule {
        span: FileSpan,
        module: ModuleName,
    },
    UnknownName {
        span: FileSpan,
        kind: ResolveNameKind,
        name: String,
    },
    DuplicateDefinition {
        span: FileSpan,
        previous: Option<FileSpan>,
        kind: ResolveNameKind,
        name: Symbol,
    },
    ClassArity {
        span: FileSpan,
        class_name: String,
        actual: usize,
    },
    SignatureArity {
        span: FileSpan,
        name: Symbol,
        params: usize,
    },
    FixityConflict {
        span: FileSpan,
        left: Symbol,
        right: Symbol,
    },
    IllegalUnaryMinus {
        span: FileSpan,
    },
    MalformedOperatorExpression {
        span: FileSpan,
        message: String,
    },
    CyclicImports {
        span: FileSpan,
        modules: Vec<ModuleName>,
    },
}

impl ResolveError {
    pub fn to_diagnostic(&self, session: &Session, modules: Option<&ModuleTable>) -> Diagnostic {
        match self {
            Self::AmbiguousImport {
                span,
                kind,
                name,
                candidates,
            } => {
                let mut notes = Vec::new();
                if let Some(modules) = modules {
                    let rendered = candidates
                        .iter()
                        .map(|candidate| modules.def_path(*candidate).render(session))
                        .collect::<Vec<_>>()
                        .join(", ");

                    if !rendered.is_empty() {
                        notes.push(format!("candidates: {rendered}"));
                    }
                }

                Diagnostic::error(
                    "RES0001",
                    format!(
                        "ambiguous imported {} `{}`",
                        kind.as_str(),
                        session.str(*name)
                    ),
                )
                .with_label(Label::primary(
                    *span,
                    format!(
                        "`{}` is provided by more than one imported module",
                        session.str(*name)
                    ),
                ))
                .with_notes(notes)
            }
            Self::UnknownModule { span, module } => Diagnostic::error(
                "RES0002",
                format!("unknown module `{}`", module.as_string(session)),
            )
            .with_label(Label::primary(
                *span,
                format!("module `{}` could not be found", module.as_string(session)),
            )),
            Self::UnknownName { span, kind, name } => {
                Diagnostic::error("RES0003", format!("unknown {} `{}`", kind.as_str(), name))
                    .with_label(Label::primary(
                        *span,
                        format!("no {} named `{}` is in scope here", kind.as_str(), name),
                    ))
            }
            Self::DuplicateDefinition {
                span,
                previous,
                kind,
                name,
            } => {
                let mut diagnostic = Diagnostic::error(
                    "RES0004",
                    format!(
                        "duplicate {} definition `{}`",
                        kind.as_str(),
                        session.str(*name)
                    ),
                )
                .with_label(Label::primary(
                    *span,
                    format!("`{}` is defined again here", session.str(*name)),
                ));

                if let Some(previous) = previous {
                    diagnostic = diagnostic.with_label(Label::secondary(
                        *previous,
                        format!("previous definition of `{}` is here", session.str(*name)),
                    ));
                }

                diagnostic
            }
            Self::ClassArity {
                span,
                class_name,
                actual,
            } => Diagnostic::error(
                "RES0005",
                format!("class `{class_name}` must have exactly one parameter"),
            )
            .with_label(Label::primary(
                *span,
                format!("expected exactly one class parameter, but found {actual}"),
            )),
            Self::SignatureArity { span, name, params } => Diagnostic::error(
                "RES0006",
                format!(
                    "function `{}` has more parameters than its signature accepts",
                    session.str(*name)
                ),
            )
            .with_label(Label::primary(
                *span,
                format!(
                    "this definition has {} parameter{}, but the signature is not a function type",
                    params,
                    if *params == 1 { "" } else { "s" }
                ),
            )),
            Self::FixityConflict { span, left, right } => Diagnostic::error(
                "RES0008",
                "conflicting operator fixities",
            )
            .with_label(Label::primary(
                *span,
                format!(
                    "operators `{}` and `{}` have the same precedence but cannot be mixed without parentheses",
                    session.str(*left),
                    session.str(*right)
                ),
            )),
            Self::IllegalUnaryMinus { span } => Diagnostic::error(
                "RES0009",
                "illegal unary minus",
            )
            .with_label(Label::primary(
                *span,
                "prefix `-` is not allowed as the right operand of this operator",
            ))
            .with_notes(vec![
                "add parentheses if the negative expression is intended here".to_string(),
            ]),
            Self::MalformedOperatorExpression { span, message } => Diagnostic::error(
                "RES0010",
                "malformed operator expression",
            )
            .with_label(Label::primary(*span, message.clone())),
            Self::CyclicImports { span, modules } => {
                Diagnostic::error("RES0007", "cyclic module imports").with_label(Label::primary(
                    *span,
                    format!(
                        "cycle: {}",
                        modules
                            .iter()
                            .map(|module| module.as_string(session))
                            .collect::<Vec<_>>()
                            .join(" -> ")
                    ),
                ))
            }
        }
    }
}

impl<'a> IntoDiagnostic<(&'a Session, Option<&'a ModuleTable>)> for ResolveError {
    fn into_diagnostic(
        self,
        (session, modules): (&'a Session, Option<&'a ModuleTable>),
    ) -> Diagnostic {
        self.to_diagnostic(session, modules)
    }
}

impl<'a> IntoDiagnostic<(&'a Session, Option<&'a ModuleTable>)> for &ResolveError {
    fn into_diagnostic(
        self,
        (session, modules): (&'a Session, Option<&'a ModuleTable>),
    ) -> Diagnostic {
        self.to_diagnostic(session, modules)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModuleApi {
    pub terms: FxHashMap<Symbol, DefId>,
    pub types: FxHashMap<Symbol, DefId>,
    pub constructors: FxHashMap<Symbol, DefId>,
    pub classes: FxHashMap<Symbol, DefId>,
    pub fixities: FxHashMap<Symbol, Fixity>,

    pub class_methods: FxHashMap<Symbol, Vec<Symbol>>,
}
impl ModuleApi {
    pub fn filtered(&self, spec: &ImportSpec) -> Self {
        match spec {
            ImportSpec::All => self.clone(),

            ImportSpec::Only(names) => {
                let mut out = Self::default();

                for name in names {
                    if let Some(sym) = self.terms.get(name) {
                        out.terms.insert(*name, *sym);
                        if let Some(fixity) = self.fixities.get(name) {
                            out.fixities.insert(*name, *fixity);
                        }
                    }

                    if let Some(sym) = self.types.get(name) {
                        out.types.insert(*name, *sym);
                    }

                    if let Some(sym) = self.constructors.get(name) {
                        out.constructors.insert(*name, *sym);
                    }

                    if let Some(sym) = self.classes.get(name) {
                        out.classes.insert(*name, *sym);

                        // Solveig's current simple import rule is:
                        //
                        //   import M(Eq)
                        //
                        // makes the class and its methods available. We do not have
                        // Haskell-style C / C(..) / C(method) import syntax yet, so
                        // selecting a class imports all of its method terms.
                        if let Some(method_names) = self.class_methods.get(name) {
                            out.class_methods.insert(*name, method_names.clone());

                            for method_name in method_names {
                                if let Some(method_sym) = self.terms.get(method_name) {
                                    out.terms.insert(*method_name, *method_sym);
                                    if let Some(fixity) = self.fixities.get(method_name) {
                                        out.fixities.insert(*method_name, *fixity);
                                    }
                                }
                            }
                        }
                    }
                }

                out
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TypedModuleApi {
    pub term_schemes: FxHashMap<DefId, solveig_ast::ast::Scheme>,
    pub type_kinds: FxHashMap<DefId, Kind>,
    pub type_ids: FxHashMap<DefId, solveig_ast::ast::TyConId>,
    pub constructor_types: FxHashMap<DefId, Ty>,
    pub constructor_ids: FxHashMap<DefId, CtorId>,
    pub class_defs: FxHashMap<DefId, solveig_ast::ast::ClassDefinition>,
    pub tycon_table: TyConTable,
    pub ctor_table: CtorTable,
    pub class_table: ClassTable,
    pub instance_defs: Vec<
        solveig_ast::ast::InstanceDefinition<
            solveig_check::inference::type_infer::TcMetadata,
            std::ops::Range<usize>,
        >,
    >,
}

impl TypedModuleApi {
    fn include_class_with_superclasses(&self, out: &mut Self, sym: &DefId) {
        if out.class_defs.contains_key(sym) {
            return;
        }

        let Some(class_def) = self.class_defs.get(sym) else {
            return;
        };

        out.class_defs.insert(*sym, class_def.clone());

        // Keep superclass definitions from the same typed API if they are present.
        // This is what lets `import M(BoundedLike)` internally retain `OrdLike`
        // and `EqLike` when all three are defined in M.
        //
        // If a superclass comes from another module, that module must still be
        // imported in the usual way. Prelude classes are already imported
        // implicitly for ordinary user modules.
        for superclass in &class_def.superclasses {
            let Some((super_sym, _)) = self
                .class_defs
                .iter()
                .find(|(_, candidate)| candidate.id == superclass.class_id)
            else {
                continue;
            };

            self.include_class_with_superclasses(out, super_sym);
        }
    }

    fn include_class_methods(&self, out: &mut Self, class_def: &ClassDefinition) {
        for method in &class_def.methods {
            if let Some(scheme) = self.term_schemes.get(&method.id) {
                out.term_schemes.insert(method.id, scheme.clone());
            }
        }
    }

    pub fn filtered(&self, spec: &ImportSpec) -> Self {
        match spec {
            ImportSpec::All => self.clone(),

            ImportSpec::Only(names) => {
                let mut out = Self {
                    tycon_table: self.tycon_table.clone(),
                    ctor_table: self.ctor_table.clone(),
                    class_table: self.class_table.clone(),
                    ..Self::default()
                };

                for name in names {
                    for (sym, scheme) in &self.term_schemes {
                        if &sym.name == name {
                            out.term_schemes.insert(*sym, scheme.clone());
                        }
                    }

                    for (sym, kind) in &self.type_kinds {
                        if &sym.name == name {
                            out.type_kinds.insert(*sym, kind.clone());
                            if let Some(id) = self.type_ids.get(sym) {
                                out.type_ids.insert(*sym, *id);
                            }
                        }
                    }

                    for (sym, ty) in &self.constructor_types {
                        if &sym.name == name {
                            out.constructor_types.insert(*sym, ty.clone());
                            if let Some(id) = self.constructor_ids.get(sym) {
                                out.constructor_ids.insert(*sym, *id);
                            }
                        }
                    }
                }

                let mut selected_classes = Vec::<(DefId, bool)>::new();

                for (sym, class_def) in &self.class_defs {
                    let class_selected = names.iter().any(|name| name == &sym.name);

                    let method_selected = class_def
                        .methods
                        .iter()
                        .any(|method| names.iter().any(|name| name == &method.id.name));

                    if class_selected || method_selected {
                        // If the class itself was selected, expose its method terms too.
                        // If only one method was selected, keep the owning class definition
                        // internally, but do not implicitly expose the rest of the methods.
                        selected_classes.push((*sym, class_selected));
                    }
                }

                for (sym, include_methods) in selected_classes {
                    self.include_class_with_superclasses(&mut out, &sym);

                    if include_methods && let Some(class_def) = self.class_defs.get(&sym) {
                        self.include_class_methods(&mut out, class_def);
                    }
                }

                // Importing a module makes its instances available. The explicit
                // import list controls names, not instance availability.
                out.instance_defs = self.instance_defs.clone();

                out
            }
        }
    }

    pub fn extend_display_maps(&self, out: &mut TypedDisplayMaps<'_>) {
        for (sym, scheme) in &self.term_schemes {
            out.func_schemes.insert(*sym, scheme.clone());
        }

        for (sym, kind) in &self.type_kinds {
            let id = self
                .type_ids
                .get(sym)
                .copied()
                .expect("ICE: typed module API type kind missing TyConId");
            out.tycon_kinds.insert(id, kind.clone());
        }

        for (sym, ty) in &self.constructor_types {
            let id = self
                .constructor_ids
                .get(sym)
                .copied()
                .expect("ICE: typed module API constructor missing CtorId");
            out.constructor_types.insert(id, ty.clone());
        }

        for class_def in self.class_defs.values() {
            out.class_defs.insert(class_def.id, class_def.clone());
        }

        for (id, info) in self.tycon_table.iter() {
            out.tycon_table.insert_with_id(id, info.clone());
        }

        for (id, info) in self.ctor_table.iter() {
            out.ctor_table.insert_with_id(id, info.clone());
        }

        for (id, info) in self.class_table.iter() {
            out.class_table.insert_with_id(id, info.clone());
        }

        out.instance_defs.extend(self.instance_defs.clone());
    }

    pub fn type_name(&self, names: &Names, sym: DefId) -> Option<String> {
        let id = self.type_ids.get(&sym)?;
        let info = self.tycon_table.get(*id)?;
        Some(names.symbol_display_name(info.name))
    }
}

pub struct TypedDisplayMaps<'a> {
    pub func_schemes: &'a mut FxHashMap<DefId, solveig_ast::ast::Scheme>,
    pub tycon_kinds: &'a mut FxHashMap<solveig_ast::ast::TyConId, Kind>,
    pub constructor_types: &'a mut FxHashMap<CtorId, Ty>,
    pub class_defs: &'a mut FxHashMap<ClassId, solveig_ast::ast::ClassDefinition>,
    pub tycon_table: &'a mut TyConTable,
    pub ctor_table: &'a mut CtorTable,
    pub class_table: &'a mut ClassTable,
    pub instance_defs: &'a mut Vec<
        solveig_ast::ast::InstanceDefinition<
            solveig_check::inference::type_infer::TcMetadata,
            std::ops::Range<usize>,
        >,
    >,
}

pub type SharedModuleApi = Rc<ModuleApi>;
pub type SharedTypedModuleApi = Rc<TypedModuleApi>;

#[derive(Debug, Clone)]
pub struct Imported<T> {
    pub alias: Symbol,
    pub module: ModuleName,
    pub api: Rc<T>,
    pub spec: ImportSpec,
}

pub type ImportedApi = Imported<ModuleApi>;
pub type ImportedTypedApi = Imported<TypedModuleApi>;

const COMPILER_PRELUDE_BUILTINS_SEGMENTS: [&str; 2] = ["prelude", "builtins"];
const COMPILER_PRELUDE_PRIM_SEGMENTS: [&str; 2] = ["prelude", "prim"];

#[derive(Debug, Clone, Default)]
pub struct BuiltinApi {
    pub terms: FxHashMap<Symbol, DefId>,
    pub types: FxHashMap<Symbol, DefId>,
    pub constructors: FxHashMap<Symbol, DefId>,
    pub classes: FxHashMap<Symbol, DefId>,
    pub type_ids: FxHashMap<DefId, TyConId>,
    pub constructor_ids: FxHashMap<DefId, CtorId>,
    pub builtin_defs: Vec<BuiltinDefinition>,
}

pub fn compiler_prelude_builtins_module(session: &Session) -> ModuleName {
    ModuleName(
        COMPILER_PRELUDE_BUILTINS_SEGMENTS
            .iter()
            .map(|segment| session.intern(segment))
            .collect(),
    )
}

pub fn compiler_prelude_prim_module(session: &Session) -> ModuleName {
    ModuleName(
        COMPILER_PRELUDE_PRIM_SEGMENTS
            .iter()
            .map(|segment| session.intern(segment))
            .collect(),
    )
}

pub fn compiler_builtin_api(session: &Session, modules: &ModuleTable) -> BuiltinApi {
    let mut api = BuiltinApi::default();
    let prelude_builtins = compiler_prelude_builtins_module(session);
    let Some(module_id) = modules.id(&prelude_builtins) else {
        return api;
    };

    for (name, id) in compiler_type_entries() {
        let symbol = session.intern(name);
        api.type_ids.insert(local_def_id(module_id, symbol), id);
    }

    for (name, id) in compiler_constructor_entries() {
        let symbol = session.intern(name);
        api.constructor_ids
            .insert(local_def_id(module_id, symbol), id);
    }

    api
}

pub fn seed_compiler_prelude_module_api(
    session: &Session,
    module_name: &ModuleName,
    module_id: ModuleId,
    builtins: &BuiltinApi,
    api: &mut ModuleApi,
) {
    if module_name == &compiler_prelude_builtins_module(session) {
        for (name, _id) in compiler_type_entries() {
            let symbol = session.intern(name);
            api.types.insert(symbol, local_def_id(module_id, symbol));
        }

        for (name, _id) in compiler_constructor_entries() {
            let symbol = session.intern(name);
            api.constructors
                .insert(symbol, local_def_id(module_id, symbol));
        }
    }

    for builtin in &builtins.builtin_defs {
        if builtin.id.module == module_id {
            api.terms.insert(builtin.id.name, builtin.id);
        }
    }
}

fn compiler_type_entries() -> [(&'static str, TyConId); 6] {
    [
        ("Int", PRIM_TYS.int),
        ("Float", PRIM_TYS.float),
        ("Char", PRIM_TYS.char),
        ("String", PRIM_TYS.string),
        ("IO", PRIM_TYS.io),
        ("Bool", PRIM_TYS.bool),
    ]
}

fn compiler_constructor_entries() -> [(&'static str, CtorId); 2] {
    [("True", PRIM_TYS.true_ctor), ("False", PRIM_TYS.false_ctor)]
}

pub(crate) fn local_def_id(module_id: ModuleId, name: Symbol) -> DefId {
    DefId {
        module: module_id,
        name,
    }
}
