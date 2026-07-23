use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};

use solveig_ast::ast::{ClassDefinition, Kind, TypeExpr};
use solveig_base::{Diagnostic, FileId, FileSpan, IntoDiagnostic, Label, Session, Span, Symbol};
use solveig_check::inference::type_infer::CheckedModule;
use solveig_syntax::ast::{ImportDecl, ImportSpec, ModuleName, ParsedModule, SurfaceDefinition};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId {
    pub module: ModuleId,
    pub name: Symbol,
}

#[derive(Debug, Clone)]
pub struct ModuleTable {
    names: Vec<ModuleName>,
    ids: HashMap<ModuleName, ModuleId>,
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
}

pub fn defid_to_legacy_string(session: &Session, modules: &ModuleTable, def: DefId) -> String {
    let module = modules.name(def.module);

    if module.0.is_empty() {
        session.str(def.name).to_string()
    } else {
        format!("{}.{}", module.as_string(session), session.str(def.name))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveNameKind {
    Term,
    TypeConstructor,
    Constructor,
    Class,
    Signature,
}

impl ResolveNameKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Term => "term",
            Self::TypeConstructor => "type constructor",
            Self::Constructor => "constructor",
            Self::Class => "class",
            Self::Signature => "signature",
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
                        .map(|candidate| defid_to_legacy_string(session, modules, *candidate))
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
                format!("expected exactly one class parameter, but found {}", actual),
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
    pub terms: HashMap<Symbol, DefId>,
    pub types: HashMap<Symbol, DefId>,
    pub constructors: HashMap<Symbol, DefId>,
    pub classes: HashMap<Symbol, DefId>,

    pub class_methods: HashMap<Symbol, Vec<Symbol>>,
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
    pub term_schemes: HashMap<DefId, solveig_ast::ast::Scheme>,
    pub type_kinds: HashMap<DefId, Kind>,
    pub constructor_types: HashMap<DefId, TypeExpr>,
    pub class_defs: HashMap<DefId, solveig_ast::ast::ClassDefinition>,
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
                .find(|(_, candidate)| candidate.name == superclass.class_name)
            else {
                continue;
            };

            self.include_class_with_superclasses(out, super_sym);
        }
    }

    fn include_class_methods(
        &self,
        session: &Session,
        out: &mut Self,
        class_def: &ClassDefinition,
    ) {
        for method in &class_def.methods {
            let short_name = method.name.rsplit('.').next().unwrap_or(&method.name);

            for (sym, scheme) in &self.term_schemes {
                if session.str(sym.name) == short_name {
                    out.term_schemes.insert(*sym, scheme.clone());
                }
            }
        }
    }

    pub fn filtered(&self, session: &Session, spec: &ImportSpec) -> Self {
        match spec {
            ImportSpec::All => self.clone(),

            ImportSpec::Only(names) => {
                let mut out = Self::default();

                for name in names {
                    for (sym, scheme) in &self.term_schemes {
                        if &sym.name == name {
                            out.term_schemes.insert(*sym, scheme.clone());
                        }
                    }

                    for (sym, kind) in &self.type_kinds {
                        if &sym.name == name {
                            out.type_kinds.insert(*sym, kind.clone());
                        }
                    }

                    for (sym, ty) in &self.constructor_types {
                        if &sym.name == name {
                            out.constructor_types.insert(*sym, ty.clone());
                        }
                    }
                }

                let mut selected_classes = Vec::<(DefId, bool)>::new();

                for (sym, class_def) in &self.class_defs {
                    let class_selected = names.iter().any(|name| name == &sym.name);

                    let method_selected = class_def.methods.iter().any(|method| {
                        let short_name = method.name.rsplit('.').next().unwrap_or(&method.name);

                        names.iter().any(|name| session.str(*name) == short_name)
                    });

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
                        self.include_class_methods(session, &mut out, class_def);
                    }
                }

                // Importing a module makes its instances available. The explicit
                // import list controls names, not instance availability.
                out.instance_defs = self.instance_defs.clone();

                out
            }
        }
    }

    pub fn extend_display_maps(
        &self,
        session: &Session,
        modules: &ModuleTable,
        out: &mut TypedDisplayMaps<'_>,
    ) {
        for (sym, scheme) in &self.term_schemes {
            out.func_schemes.insert(
                defid_to_legacy_string(session, modules, *sym),
                scheme.clone(),
            );
        }

        for (sym, kind) in &self.type_kinds {
            out.tycon_kinds
                .insert(defid_to_legacy_string(session, modules, *sym), kind.clone());
        }

        for (sym, ty) in &self.constructor_types {
            out.constructor_types
                .insert(defid_to_legacy_string(session, modules, *sym), ty.clone());
        }

        for (sym, class_def) in &self.class_defs {
            out.class_defs.insert(
                defid_to_legacy_string(session, modules, *sym),
                class_def.clone(),
            );
        }

        out.instance_defs.extend(self.instance_defs.clone());
    }
}

pub struct TypedDisplayMaps<'a> {
    pub func_schemes: &'a mut HashMap<String, solveig_ast::ast::Scheme>,
    pub tycon_kinds: &'a mut HashMap<String, Kind>,
    pub constructor_types: &'a mut HashMap<String, TypeExpr>,
    pub class_defs: &'a mut HashMap<String, solveig_ast::ast::ClassDefinition>,
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

#[derive(Debug, Clone, Default)]
pub struct BuiltinApi {
    pub terms: HashMap<Symbol, DefId>,
    pub types: HashMap<Symbol, DefId>,
    pub constructors: HashMap<Symbol, DefId>,
    pub classes: HashMap<Symbol, DefId>,
}

#[derive(Debug, Clone)]
pub struct ModuleContext {
    pub module: ModuleName,
    pub module_id: ModuleId,
    pub file: FileId,
    pub local_api: SharedModuleApi,
    pub imported_apis: Vec<ImportedApi>,
    pub builtins: Rc<BuiltinApi>,
}

#[derive(Debug)]
pub struct ModuleFile {
    pub name: ModuleName,
    pub path: PathBuf,
    pub imports: Vec<ImportDecl>,
    pub parsed: ParsedModule,
}

#[derive(Debug)]
pub struct Project {
    pub modules: HashMap<ModuleName, ModuleFile>,
    pub module_table: ModuleTable,
}

pub struct ParsedProject {
    pub modules: HashMap<ModuleName, ParsedModule>,
}

impl ParsedProject {
    pub fn get(&self, session: &Session, module: &ModuleName) -> &ParsedModule {
        self.modules
            .get(module)
            .unwrap_or_else(|| panic!("ICE: missing parsed module {}", module.as_string(session)))
    }
}

#[derive(Debug)]
pub struct ModuleGraph {
    pub graph: DiGraph<ModuleName, ()>,
    pub indices: HashMap<ModuleName, NodeIndex>,
    pub import_spans: HashMap<(ModuleName, ModuleName), FileSpan>,
}

#[derive(Debug, Clone)]
pub struct EffectiveImport {
    pub alias: Symbol,
    pub module: ModuleName,
    pub spec: ImportSpec,
    pub span: FileSpan,
}

pub fn is_prelude_module(session: &Session, module: &ModuleName) -> bool {
    module.as_string(session).starts_with("prelude.")
}

fn implicit_prelude_modules(session: &Session, module: &ModuleName) -> Vec<ModuleName> {
    if is_prelude_module(session, module) {
        vec![]
    } else {
        vec![
            ModuleName(vec![session.intern("prelude"), session.intern("builtins")]),
            ModuleName(vec![session.intern("prelude"), session.intern("list")]),
            ModuleName(vec![
                session.intern("prelude"),
                session.intern("typeclasses"),
            ]),
        ]
    }
}

pub fn effective_imports(
    session: &Session,
    module: &ModuleName,
    explicit: &[ImportDecl],
    file: FileId,
) -> Vec<EffectiveImport> {
    let mut out = explicit
        .iter()
        .map(|imp| EffectiveImport {
            alias: imp.alias,
            module: imp.module.clone(),
            spec: imp.spec.clone(),
            span: FileSpan::new(file, imp.span),
        })
        .collect::<Vec<_>>();

    for implicit in implicit_prelude_modules(session, module) {
        out.push(EffectiveImport {
            alias: implicit.default_alias(),
            module: implicit,
            spec: ImportSpec::All,
            span: FileSpan::new(file, Span::new(0, 0)),
        });
    }

    out
}

pub fn build_module_graph(
    session: &Session,
    project: &Project,
) -> Result<ModuleGraph, ResolveError> {
    let mut graph = DiGraph::<ModuleName, ()>::new();
    let mut indices = HashMap::new();
    let mut import_spans = HashMap::new();

    for module_name in project.modules.keys() {
        let ix = graph.add_node(module_name.clone());
        indices.insert(module_name.clone(), ix);
    }

    for (module_name, module_file) in &project.modules {
        let from = *indices
            .get(module_name)
            .expect("ICE: module graph missing node for known module");

        for imp in effective_imports(
            session,
            module_name,
            &module_file.imports,
            module_file.parsed.file,
        ) {
            let to = *indices
                .get(&imp.module)
                .ok_or_else(|| ResolveError::UnknownModule {
                    span: imp.span,
                    module: imp.module.clone(),
                })?;
            graph.add_edge(from, to, ());
            import_spans.insert((module_name.clone(), imp.module), imp.span);
        }
    }

    Ok(ModuleGraph {
        graph,
        indices,
        import_spans,
    })
}

pub fn module_order_checked(
    session: &Session,
    project: &Project,
) -> Result<Vec<ModuleName>, ResolveError> {
    let graph = build_module_graph(session, project)?;
    ensure_acyclic_modules(session, &graph)?;
    module_order(session, &graph)
}

pub fn ensure_acyclic_modules(session: &Session, graph: &ModuleGraph) -> Result<(), ResolveError> {
    for scc in tarjan_scc(&graph.graph) {
        if scc.len() > 1 {
            let mut modules = scc
                .iter()
                .map(|ix| graph.graph[*ix].clone())
                .collect::<Vec<_>>();
            modules.sort_by_key(|module| module.as_string(session));

            let span = cycle_span(graph, &modules)
                .expect("ICE: cyclic import diagnostic requires an import edge span");

            return Err(ResolveError::CyclicImports { span, modules });
        }

        let ix = scc[0];
        if graph.graph.contains_edge(ix, ix) {
            let module = graph.graph[ix].clone();
            let span = graph
                .import_spans
                .get(&(module.clone(), module.clone()))
                .copied()
                .expect("ICE: self import diagnostic requires an import edge span");

            return Err(ResolveError::CyclicImports {
                span,
                modules: vec![module],
            });
        }
    }
    Ok(())
}

fn cycle_span(graph: &ModuleGraph, modules: &[ModuleName]) -> Option<FileSpan> {
    for from in modules {
        for to in modules {
            if let Some(span) = graph.import_spans.get(&(from.clone(), to.clone())) {
                return Some(*span);
            }
        }
    }

    None
}

pub fn module_order(
    _session: &Session,
    graph: &ModuleGraph,
) -> Result<Vec<ModuleName>, ResolveError> {
    let mut order = toposort(&graph.graph, None)
        .map_err(|cycle| {
            let module = graph.graph[cycle.node_id()].clone();
            let span = graph
                .import_spans
                .iter()
                .find_map(|((from, to), span)| {
                    if from == &module || to == &module {
                        Some(*span)
                    } else {
                        None
                    }
                })
                .expect("ICE: toposort cycle diagnostic requires an import edge span");

            ResolveError::CyclicImports {
                span,
                modules: vec![module],
            }
        })?
        .into_iter()
        .map(|ix| graph.graph[ix].clone())
        .collect::<Vec<_>>();

    order.reverse();
    Ok(order)
}

fn local_def_id(module_id: ModuleId, name: Symbol) -> DefId {
    DefId {
        module: module_id,
        name,
    }
}

fn file_span(file: FileId, span: Span) -> FileSpan {
    FileSpan::new(file, span)
}

fn check_duplicate(
    seen: &mut HashMap<Symbol, FileSpan>,
    file: FileId,
    span: Span,
    kind: ResolveNameKind,
    name: Symbol,
) -> Result<(), ResolveError> {
    let current = file_span(file, span);
    if let Some(previous) = seen.insert(name, current) {
        return Err(ResolveError::DuplicateDefinition {
            span: current,
            previous: Some(previous),
            kind,
            name,
        });
    }

    Ok(())
}

pub fn build_module_api(
    module_id: ModuleId,
    parsed: &ParsedModule,
) -> Result<ModuleApi, ResolveError> {
    let mut api = ModuleApi::default();
    let mut term_defs = HashMap::<Symbol, FileSpan>::new();
    let mut term_sigs = HashMap::<Symbol, FileSpan>::new();
    let mut types = HashMap::<Symbol, FileSpan>::new();
    let mut seen_constructors = HashMap::<Symbol, FileSpan>::new();
    let mut classes = HashMap::<Symbol, FileSpan>::new();

    for def in &parsed.defs {
        match &def.0 {
            SurfaceDefinition::FunDef { name, .. } => {
                check_duplicate(
                    &mut term_defs,
                    parsed.file,
                    name.1,
                    ResolveNameKind::Term,
                    name.0,
                )?;

                api.terms.insert(name.0, local_def_id(module_id, name.0));
            }

            SurfaceDefinition::SigDef { name, .. } => {
                check_duplicate(
                    &mut term_sigs,
                    parsed.file,
                    name.1,
                    ResolveNameKind::Signature,
                    name.0,
                )?;

                api.terms
                    .entry(name.0)
                    .or_insert(local_def_id(module_id, name.0));
            }

            SurfaceDefinition::DataDef {
                name, constructors, ..
            } => {
                check_duplicate(
                    &mut types,
                    parsed.file,
                    name.1,
                    ResolveNameKind::TypeConstructor,
                    name.0,
                )?;

                api.types.insert(name.0, local_def_id(module_id, name.0));

                for ctor in constructors {
                    check_duplicate(
                        &mut seen_constructors,
                        parsed.file,
                        ctor.0.name.1,
                        ResolveNameKind::Constructor,
                        ctor.0.name.0,
                    )?;

                    api.constructors
                        .insert(ctor.0.name.0, local_def_id(module_id, ctor.0.name.0));
                }
            }

            SurfaceDefinition::ClassDef { name, methods, .. } => {
                let class_name = name.0;
                check_duplicate(
                    &mut classes,
                    parsed.file,
                    name.1,
                    ResolveNameKind::Class,
                    class_name,
                )?;

                api.classes
                    .insert(class_name, local_def_id(module_id, class_name));

                let mut method_names = Vec::new();

                for method in methods {
                    let method_name = method.name.0;
                    check_duplicate(
                        &mut term_defs,
                        parsed.file,
                        method.name.1,
                        ResolveNameKind::Term,
                        method_name,
                    )?;

                    api.terms
                        .entry(method_name)
                        .or_insert(local_def_id(module_id, method_name));

                    method_names.push(method_name);
                }

                api.class_methods.insert(class_name, method_names);
            }

            SurfaceDefinition::InstanceDef { .. } => {}
        }
    }

    Ok(api)
}

pub fn build_typed_module_api(
    session: &Session,
    module_id: ModuleId,
    module_name: &ModuleName,
    checked: &CheckedModule,
) -> TypedModuleApi {
    let mut api = TypedModuleApi::default();
    let prefix = module_name.as_string(session) + ".";

    for (name, scheme) in &checked.func_schemes {
        if let Some(stripped) = name.strip_prefix(&prefix) {
            api.term_schemes.insert(
                local_def_id(module_id, session.intern(stripped)),
                scheme.clone(),
            );
        }
    }

    for (name, kind) in &checked.tycon_kinds {
        if let Some(stripped) = name.strip_prefix(&prefix) {
            api.type_kinds.insert(
                local_def_id(module_id, session.intern(stripped)),
                kind.clone(),
            );
        }
    }

    for (name, ty) in &checked.constructor_types {
        if let Some(stripped) = name.strip_prefix(&prefix) {
            api.constructor_types.insert(
                local_def_id(module_id, session.intern(stripped)),
                ty.clone(),
            );
        }
    }

    for class_def in &checked.class_defs {
        let stripped = class_def
            .name
            .strip_prefix(&prefix)
            .unwrap_or(&class_def.name);

        api.class_defs.insert(
            local_def_id(module_id, session.intern(stripped)),
            class_def.clone(),
        );
    }

    api.instance_defs = checked.instance_defs.clone();

    api
}
