use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};

use base::{FileId, Session, Symbol};
use core::ast::{ClassDefinition, Kind, TypeExpr};
use surface::ast::{ImportDecl, ImportSpec, ModuleName, ParsedModule, SurfaceDefinition};
use surface::parser;
use syntax::inference::type_infer::CheckedModule;

const PRELUDE_BUILTINS_SRC: &str = include_str!("./prelude/builtins.sv");
const PRELUDE_LIST_SRC: &str = include_str!("./prelude/list.sv");
const PRELUDE_TYPECLASS_SRC: &str = include_str!("./prelude/typeclasses.sv");

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolId {
    pub module: ModuleName,
    pub name: Symbol,
}

impl SymbolId {
    pub fn display(&self, session: &Session) -> String {
        if self.module.0.is_empty() {
            session.str(self.name).to_string()
        } else {
            format!(
                "{}.{}",
                self.module.as_string(session),
                session.str(self.name)
            )
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModuleApi {
    pub terms: HashMap<Symbol, SymbolId>,
    pub types: HashMap<Symbol, SymbolId>,
    pub constructors: HashMap<Symbol, SymbolId>,
    pub classes: HashMap<Symbol, SymbolId>,

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
                        out.terms.insert(*name, sym.clone());
                    }

                    if let Some(sym) = self.types.get(name) {
                        out.types.insert(*name, sym.clone());
                    }

                    if let Some(sym) = self.constructors.get(name) {
                        out.constructors.insert(*name, sym.clone());
                    }

                    if let Some(sym) = self.classes.get(name) {
                        out.classes.insert(*name, sym.clone());

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
                                    out.terms.insert(*method_name, method_sym.clone());
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
    pub term_schemes: HashMap<SymbolId, core::ast::Scheme>,
    pub type_kinds: HashMap<SymbolId, Kind>,
    pub constructor_types: HashMap<SymbolId, TypeExpr>,
    pub class_defs: HashMap<SymbolId, core::ast::ClassDefinition>,
    pub instance_defs: Vec<
        core::ast::InstanceDefinition<
            syntax::inference::type_infer::TcMetadata,
            std::ops::Range<usize>,
        >,
    >,
}

impl TypedModuleApi {
    fn include_class_with_superclasses(&self, out: &mut Self, sym: &SymbolId) {
        if out.class_defs.contains_key(sym) {
            return;
        }

        let Some(class_def) = self.class_defs.get(sym) else {
            return;
        };

        out.class_defs.insert(sym.clone(), class_def.clone());

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
                    out.term_schemes.insert(sym.clone(), scheme.clone());
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
                            out.term_schemes.insert(sym.clone(), scheme.clone());
                        }
                    }

                    for (sym, kind) in &self.type_kinds {
                        if &sym.name == name {
                            out.type_kinds.insert(sym.clone(), kind.clone());
                        }
                    }

                    for (sym, ty) in &self.constructor_types {
                        if &sym.name == name {
                            out.constructor_types.insert(sym.clone(), ty.clone());
                        }
                    }
                }

                let mut selected_classes = Vec::<(SymbolId, bool)>::new();

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
                        selected_classes.push((sym.clone(), class_selected));
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
        func_schemes: &mut HashMap<String, core::ast::Scheme>,
        tycon_kinds: &mut HashMap<String, Kind>,
        constructor_types: &mut HashMap<String, TypeExpr>,
        class_defs: &mut HashMap<String, core::ast::ClassDefinition>,
        instance_defs: &mut Vec<
            core::ast::InstanceDefinition<
                syntax::inference::type_infer::TcMetadata,
                std::ops::Range<usize>,
            >,
        >,
    ) {
        for (sym, scheme) in &self.term_schemes {
            func_schemes.insert(sym.display(session), scheme.clone());
        }

        for (sym, kind) in &self.type_kinds {
            tycon_kinds.insert(sym.display(session), kind.clone());
        }

        for (sym, ty) in &self.constructor_types {
            constructor_types.insert(sym.display(session), ty.clone());
        }

        for (sym, class_def) in &self.class_defs {
            class_defs.insert(sym.display(session), class_def.clone());
        }

        instance_defs.extend(self.instance_defs.clone());
    }
}

pub type SharedModuleApi = Rc<ModuleApi>;
pub type SharedTypedModuleApi = Rc<TypedModuleApi>;

#[derive(Debug, Clone)]
pub struct Imported<T> {
    pub alias: Symbol,
    pub api: Rc<T>,
    pub spec: ImportSpec,
}

pub type ImportedApi = Imported<ModuleApi>;
pub type ImportedTypedApi = Imported<TypedModuleApi>;

#[derive(Debug, Clone, Default)]
pub struct BuiltinApi {
    pub terms: HashMap<Symbol, SymbolId>,
    pub types: HashMap<Symbol, SymbolId>,
    pub constructors: HashMap<Symbol, SymbolId>,
    pub classes: HashMap<Symbol, SymbolId>,
}

#[derive(Debug, Clone)]
pub struct ModuleContext {
    pub module: ModuleName,
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
}

#[derive(Debug, Clone)]
pub struct EffectiveImport {
    pub alias: Symbol,
    pub module: ModuleName,
    pub spec: ImportSpec,
}

pub fn is_prelude_module(session: &Session, module: &ModuleName) -> bool {
    module.as_string(session).starts_with("prelude.")
}

fn prelude_module(session: &Session, module: &ModuleName) -> Option<&'static str> {
    match module.as_string(session).as_str() {
        "prelude.builtins" => Some(PRELUDE_BUILTINS_SRC),
        "prelude.list" => Some(PRELUDE_LIST_SRC),
        "prelude.typeclasses" => Some(PRELUDE_TYPECLASS_SRC),
        _ => None,
    }
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
) -> Vec<EffectiveImport> {
    let mut out = explicit
        .iter()
        .map(|imp| EffectiveImport {
            alias: imp.alias,
            module: imp.module.clone(),
            spec: imp.spec.clone(),
        })
        .collect::<Vec<_>>();

    for implicit in implicit_prelude_modules(session, module) {
        out.push(EffectiveImport {
            alias: implicit.default_alias(),
            module: implicit,
            spec: ImportSpec::All,
        });
    }

    out
}

fn parse_source(session: &Session, file: FileId, source: &str) -> ParsedModule {
    parser::parse_source(session, file, source)
}

fn mk_virtual_prelude_file(session: &Session, module: &ModuleName) -> Result<ModuleFile, String> {
    let source = prelude_module(session, module)
        .ok_or_else(|| {
            format!(
                "unknown compiler prelude module {}",
                module.as_string(session)
            )
        })?
        .to_string();

    let path = PathBuf::from(format!(
        "<compiler-prelude>/{}.sv",
        module.as_string(session)
    ));
    let file = session.add_file(path.clone(), source.clone());
    let parsed = parse_source(session, file, &source);
    let imports = parsed.imports.clone();

    Ok(ModuleFile {
        name: module.clone(),
        path,
        imports,
        parsed,
    })
}

fn scan_module_file(session: &Session, root: &Path, file: &Path) -> Result<ModuleFile, String> {
    let source = fs::read_to_string(file)
        .map_err(|e| format!("failed to read {}: {}", file.display(), e))?;

    let name = module_name_from_file(session, root, file)?;
    let file_id = session.add_file(file.to_path_buf(), source.clone());
    let parsed = parse_source(session, file_id, &source);
    let imports = parsed.imports.clone();

    Ok(ModuleFile {
        name,
        path: file.to_path_buf(),
        imports,
        parsed,
    })
}

pub fn module_name_from_file(
    session: &Session,
    root: &Path,
    file: &Path,
) -> Result<ModuleName, String> {
    let rel = file
        .strip_prefix(root)
        .map_err(|_| format!("{} is not under {}", file.display(), root.display()))?;

    let mut parts = rel
        .iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    let last = parts.last_mut().ok_or_else(|| "empty path".to_string())?;
    if let Some(stem) = last.strip_suffix(".sv") {
        *last = stem.to_string();
    } else {
        return Err(format!("expected .sv file: {}", file.display()));
    }

    Ok(ModuleName(
        parts.iter().map(|part| session.intern(part)).collect(),
    ))
}

pub fn file_from_module_name(session: &Session, root: &Path, module: &ModuleName) -> PathBuf {
    let mut out = root.to_path_buf();
    for seg in &module.0 {
        out.push(session.str(*seg));
    }
    out.set_extension("sv");
    out
}

pub fn parse_project(project: &Project) -> ParsedProject {
    let mut modules = HashMap::new();

    for (name, file) in &project.modules {
        modules.insert(name.clone(), file.parsed.clone());
    }

    ParsedProject { modules }
}

pub fn load_project(session: &Session, entry_file: &Path) -> Result<(PathBuf, Project), String> {
    let root = entry_file
        .parent()
        .ok_or_else(|| format!("cannot determine source root for {}", entry_file.display()))?
        .to_path_buf();

    let mut modules = HashMap::<ModuleName, ModuleFile>::new();
    let mut stack = vec![entry_file.to_path_buf()];

    while let Some(file) = stack.pop() {
        let module_file = scan_module_file(session, &root, &file)?;
        if modules.contains_key(&module_file.name) {
            continue;
        }

        let module_name = module_file.name.clone();

        if !session.diagnostics().has_errors() {
            for imp in effective_imports(session, &module_name, &module_file.imports) {
                if prelude_module(session, &imp.module).is_some() {
                    if !modules.contains_key(&imp.module) {
                        modules.insert(
                            imp.module.clone(),
                            mk_virtual_prelude_file(session, &imp.module)?,
                        );
                    }
                } else {
                    stack.push(file_from_module_name(session, &root, &imp.module));
                }
            }
        }

        modules.insert(module_name, module_file);
    }

    Ok((root, Project { modules }))
}

pub fn build_module_graph(session: &Session, project: &Project) -> Result<ModuleGraph, String> {
    let mut graph = DiGraph::<ModuleName, ()>::new();
    let mut indices = HashMap::new();

    for module_name in project.modules.keys() {
        let ix = graph.add_node(module_name.clone());
        indices.insert(module_name.clone(), ix);
    }

    for (module_name, module_file) in &project.modules {
        let from = indices[module_name];

        for imp in effective_imports(session, module_name, &module_file.imports) {
            let to = *indices.get(&imp.module).ok_or_else(|| {
                format!(
                    "module {} imports missing module {}",
                    module_name.as_string(session),
                    imp.module.as_string(session)
                )
            })?;
            graph.add_edge(from, to, ());
        }
    }

    Ok(ModuleGraph { graph, indices })
}

pub fn module_order_checked(
    session: &Session,
    project: &Project,
) -> Result<Vec<ModuleName>, String> {
    let graph = build_module_graph(session, project)?;
    ensure_acyclic_modules(session, &graph)?;
    module_order(session, &graph)
}

pub fn ensure_acyclic_modules(session: &Session, graph: &ModuleGraph) -> Result<(), String> {
    for scc in tarjan_scc(&graph.graph) {
        if scc.len() > 1 {
            let names = scc
                .iter()
                .map(|ix| graph.graph[*ix].as_string(session))
                .collect::<Vec<_>>();
            return Err(format!("cyclic module imports: {}", names.join(" -> ")));
        }

        let ix = scc[0];
        if graph.graph.contains_edge(ix, ix) {
            return Err(format!(
                "module {} imports itself",
                graph.graph[ix].as_string(session)
            ));
        }
    }
    Ok(())
}

pub fn module_order(session: &Session, graph: &ModuleGraph) -> Result<Vec<ModuleName>, String> {
    let mut order = toposort(&graph.graph, None)
        .map_err(|c| {
            format!(
                "cycle involving {}",
                graph.graph[c.node_id()].as_string(session)
            )
        })?
        .into_iter()
        .map(|ix| graph.graph[ix].clone())
        .collect::<Vec<_>>();

    order.reverse();
    Ok(order)
}

pub fn build_module_api(module_name: &ModuleName, parsed: &ParsedModule) -> ModuleApi {
    let mut api = ModuleApi::default();

    for def in &parsed.defs {
        match &def.0 {
            SurfaceDefinition::FunDef { name, .. } => {
                api.terms.insert(
                    name.0,
                    SymbolId {
                        module: module_name.clone(),
                        name: name.0,
                    },
                );
            }

            SurfaceDefinition::SigDef { name, .. } => {
                api.terms.entry(name.0).or_insert(SymbolId {
                    module: module_name.clone(),
                    name: name.0,
                });
            }

            SurfaceDefinition::DataDef {
                name, constructors, ..
            } => {
                api.types.insert(
                    name.0,
                    SymbolId {
                        module: module_name.clone(),
                        name: name.0,
                    },
                );

                for ctor in constructors {
                    api.constructors.insert(
                        ctor.0.name.0,
                        SymbolId {
                            module: module_name.clone(),
                            name: ctor.0.name.0,
                        },
                    );
                }
            }

            SurfaceDefinition::ClassDef { name, methods, .. } => {
                let class_name = name.0;

                api.classes.insert(
                    class_name,
                    SymbolId {
                        module: module_name.clone(),
                        name: class_name,
                    },
                );

                let mut method_names = Vec::new();

                for method in methods {
                    let method_name = method.name.0;

                    api.terms.entry(method_name).or_insert(SymbolId {
                        module: module_name.clone(),
                        name: method_name,
                    });

                    method_names.push(method_name);
                }

                api.class_methods.insert(class_name, method_names);
            }

            SurfaceDefinition::InstanceDef { .. } => {}
        }
    }

    api
}

pub fn build_typed_module_api(
    session: &Session,
    module_name: &ModuleName,
    checked: &CheckedModule,
) -> TypedModuleApi {
    let mut api = TypedModuleApi::default();
    let prefix = module_name.as_string(session) + ".";

    for (name, scheme) in &checked.func_schemes {
        if let Some(stripped) = name.strip_prefix(&prefix) {
            api.term_schemes.insert(
                SymbolId {
                    module: module_name.clone(),
                    name: session.intern(stripped),
                },
                scheme.clone(),
            );
        }
    }

    for (name, kind) in &checked.tycon_kinds {
        if let Some(stripped) = name.strip_prefix(&prefix) {
            api.type_kinds.insert(
                SymbolId {
                    module: module_name.clone(),
                    name: session.intern(stripped),
                },
                kind.clone(),
            );
        }
    }

    for (name, ty) in &checked.constructor_types {
        if let Some(stripped) = name.strip_prefix(&prefix) {
            api.constructor_types.insert(
                SymbolId {
                    module: module_name.clone(),
                    name: session.intern(stripped),
                },
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
            SymbolId {
                module: module_name.clone(),
                name: session.intern(stripped),
            },
            class_def.clone(),
        );
    }

    api.instance_defs = checked.instance_defs.clone();

    api
}
