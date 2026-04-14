use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use chumsky::{input::Input as _, Parser as _};
use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};

use surface::ast::{ImportDecl, ImportSpec, ModuleName, ParsedModule, SurfaceDefinition};
use surface::parser::{lexer, module_parser, render_rich_errors};

use core::ast::{Kind, TypeExpr};
use syntax::inference::type_infer::CheckedModule;

const PRELUDE_BUILTINS_SRC: &str = include_str!("./prelude/builtins.sv");
const PRELUDE_LIST_SRC: &str = include_str!("./prelude/list.sv");

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolId {
    pub module: ModuleName,
    pub name: String,
}

impl SymbolId {
    pub fn display(&self) -> String {
        if self.module.0.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.module.as_string(), self.name)
        }
    }
}

fn prelude_module(module: &ModuleName) -> Option<&'static str> {
    match module.as_string().as_str() {
        "prelude.builtins" => Some(PRELUDE_BUILTINS_SRC),
        "prelude.list" => Some(PRELUDE_LIST_SRC),
        _ => None,
    }
}
fn mk_virtual_prelude_file(module: &ModuleName) -> Result<ModuleFile, String> {
    let source = prelude_module(module)
        .ok_or_else(|| format!("unknown compiler prelude module {}", module.as_string()))?
        .to_string();

    let path = PathBuf::from(format!("<compiler-prelude>/{}.sv", module.as_string()));

    let tokens = lexer()
        .parse(source.as_str())
        .into_result()
        .map_err(|errs| render_rich_errors(&path, &source, "lex", &errs))?;

    let input = tokens[..].split_spanned((0..tokens.len()).into());
    let parsed = module_parser()
        .parse(input)
        .into_result()
        .map_err(|errs| render_rich_errors(&path, &source, "parse", &errs))?;

    Ok(ModuleFile {
        name: module.clone(),
        path,
        source: source.clone(),
        imports: parsed.imports.clone(),
    })
}

fn implicit_prelude_modules(module: &ModuleName) -> Vec<ModuleName> {
    if module.as_string().starts_with("prelude.") {
        vec![]
    } else {
        vec![
            ModuleName(vec!["prelude".into(), "builtins".into()]),
            ModuleName(vec!["prelude".into(), "list".into()]),
        ]
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModuleApi {
    pub terms: HashMap<String, SymbolId>,
    pub types: HashMap<String, SymbolId>,
    pub constructors: HashMap<String, SymbolId>,
}

#[derive(Debug, Clone, Default)]
pub struct TypedModuleApi {
    pub term_types: HashMap<SymbolId, TypeExpr>,
    pub type_kinds: HashMap<SymbolId, Kind>,
    pub constructor_types: HashMap<SymbolId, TypeExpr>,
}

pub type SharedModuleApi = Rc<ModuleApi>;
pub type SharedTypedModuleApi = Rc<TypedModuleApi>;

#[derive(Debug, Clone)]
pub struct ImportedApi {
    pub alias: String,
    pub api: SharedModuleApi,
    pub spec: ImportSpec,
}

#[derive(Debug, Clone)]
pub struct ImportedTypedApi {
    pub alias: String,
    pub api: SharedTypedModuleApi,
    pub spec: ImportSpec,
}

#[derive(Debug, Clone, Default)]
pub struct BuiltinApi {
    pub terms: HashMap<String, SymbolId>,
    pub types: HashMap<String, SymbolId>,
    pub constructors: HashMap<String, SymbolId>,
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
    pub source: String,
    pub imports: Vec<ImportDecl>,
}

#[derive(Debug)]
pub struct Project {
    pub modules: HashMap<ModuleName, ModuleFile>,
}

#[derive(Debug)]
pub struct ModuleGraph {
    pub graph: DiGraph<ModuleName, ()>,
    pub indices: HashMap<ModuleName, NodeIndex>,
}

pub fn build_module_graph(project: &Project) -> Result<ModuleGraph, String> {
    let mut graph = DiGraph::<ModuleName, ()>::new();
    let mut indices = HashMap::new();

    for module_name in project.modules.keys() {
        let ix = graph.add_node(module_name.clone());
        indices.insert(module_name.clone(), ix);
    }

    for (module_name, module_file) in &project.modules {
        let from = indices[module_name];

        for imp in &module_file.imports {
            let to = *indices.get(&imp.module).ok_or_else(|| {
                format!(
                    "module {} imports missing module {}",
                    module_name.as_string(),
                    imp.module.as_string()
                )
            })?;
            graph.add_edge(from, to, ());
        }

        for implicit in implicit_prelude_modules(module_name) {
            if let Some(to) = indices.get(&implicit) {
                graph.add_edge(from, *to, ());
            }
        }
    }

    Ok(ModuleGraph { graph, indices })
}

pub fn ensure_acyclic_modules(graph: &ModuleGraph) -> Result<(), String> {
    for scc in tarjan_scc(&graph.graph) {
        if scc.len() > 1 {
            let names = scc
                .iter()
                .map(|ix| graph.graph[*ix].as_string())
                .collect::<Vec<_>>();
            return Err(format!("cyclic module imports: {}", names.join(" -> ")));
        }

        let ix = scc[0];
        if graph.graph.contains_edge(ix, ix) {
            return Err(format!(
                "module {} imports itself",
                graph.graph[ix].as_string()
            ));
        }
    }
    Ok(())
}

pub fn module_order(graph: &ModuleGraph) -> Result<Vec<ModuleName>, String> {
    let mut order = toposort(&graph.graph, None)
        .map_err(|c| format!("cycle involving {}", graph.graph[c.node_id()].as_string()))?
        .into_iter()
        .map(|ix| graph.graph[ix].clone())
        .collect::<Vec<_>>();

    // A -> B means A imports B, so B must be processed first.
    order.reverse();
    Ok(order)
}

pub fn module_name_from_file(root: &Path, file: &Path) -> Result<ModuleName, String> {
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

    Ok(ModuleName(parts))
}

pub fn file_from_module_name(root: &Path, module: &ModuleName) -> PathBuf {
    let mut out = root.to_path_buf();
    for seg in &module.0 {
        out.push(seg);
    }
    out.set_extension("sv");
    out
}

fn scan_module_file(root: &Path, file: &Path) -> Result<ModuleFile, String> {
    let source = fs::read_to_string(file)
        .map_err(|e| format!("failed to read {}: {}", file.display(), e))?;

    let tokens = lexer()
        .parse(source.as_str())
        .into_result()
        .map_err(|errs| render_rich_errors(file, &source, "lex", &errs))?;

    let input = tokens[..].split_spanned((0..tokens.len()).into());
    let parsed = module_parser()
        .parse(input)
        .into_result()
        .map_err(|errs| render_rich_errors(file, &source, "parse", &errs))?;

    let name = module_name_from_file(root, file)?;

    Ok(ModuleFile {
        name,
        path: file.to_path_buf(),
        source: source.clone(),
        imports: parsed.imports.clone(),
    })
}

pub fn parse_module<'src>(file: &'src ModuleFile) -> Result<ParsedModule<'src>, String> {
    let tokens = lexer()
        .parse(file.source.as_str())
        .into_result()
        .map_err(|errs| render_rich_errors(&file.path, &file.source, "lex", &errs))?;

    let input = tokens[..].split_spanned((0..tokens.len()).into());
    let parsed = module_parser()
        .parse(input)
        .into_result()
        .map_err(|errs| render_rich_errors(&file.path, &file.source, "parse", &errs))?;

    Ok(parsed)
}

pub fn load_project(entry_file: &Path) -> Result<(PathBuf, Project), String> {
    let root = entry_file
        .parent()
        .ok_or_else(|| format!("cannot determine source root for {}", entry_file.display()))?
        .to_path_buf();

    let mut modules = HashMap::<ModuleName, ModuleFile>::new();
    let mut stack = vec![entry_file.to_path_buf()];

    while let Some(file) = stack.pop() {
        let module_file = scan_module_file(&root, &file)?;
        if modules.contains_key(&module_file.name) {
            continue;
        }

        let module_name = module_file.name.clone();

        for imp in &module_file.imports {
            if prelude_module(&imp.module).is_some() {
                if !modules.contains_key(&imp.module) {
                    let prelude_file = mk_virtual_prelude_file(&imp.module)?;
                    modules.insert(imp.module.clone(), prelude_file);
                }
            } else {
                stack.push(file_from_module_name(&root, &imp.module));
            }
        }

        for implicit in implicit_prelude_modules(&module_name) {
            if !modules.contains_key(&implicit) {
                let prelude_file = mk_virtual_prelude_file(&implicit)?;
                modules.insert(implicit.clone(), prelude_file);
            }
        }

        modules.insert(module_name, module_file);
    }

    Ok((root, Project { modules }))
}

pub fn build_module_api<'src>(
    module_name: &ModuleName,
    parsed: &ParsedModule<'src>,
) -> Result<ModuleApi, String> {
    let mut api = ModuleApi::default();

    for def in &parsed.defs {
        match &def.inner {
            SurfaceDefinition::FunDef { name, .. } => {
                api.terms.insert(
                    name.to_string(),
                    SymbolId {
                        module: module_name.clone(),
                        name: name.to_string(),
                    },
                );
            }
            SurfaceDefinition::SigDef { name, .. } => {
                api.terms.entry(name.to_string()).or_insert(SymbolId {
                    module: module_name.clone(),
                    name: name.to_string(),
                });
            }
            SurfaceDefinition::DataDef {
                name, constructors, ..
            } => {
                api.types.insert(
                    name.to_string(),
                    SymbolId {
                        module: module_name.clone(),
                        name: name.to_string(),
                    },
                );

                for ctor in constructors {
                    api.constructors.insert(
                        ctor.name.to_string(),
                        SymbolId {
                            module: module_name.clone(),
                            name: ctor.name.to_string(),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    Ok(api)
}

pub fn filter_api(api: &ModuleApi, spec: &ImportSpec) -> ModuleApi {
    match spec {
        ImportSpec::All => api.clone(),
        ImportSpec::Only(names) => {
            let mut out = ModuleApi::default();

            for name in names {
                if let Some(v) = api.terms.get(name) {
                    out.terms.insert(name.clone(), v.clone());
                }
                if let Some(v) = api.types.get(name) {
                    out.types.insert(name.clone(), v.clone());
                }
                if let Some(v) = api.constructors.get(name) {
                    out.constructors.insert(name.clone(), v.clone());
                }
            }

            out
        }
    }
}

pub fn build_typed_module_api(module_name: &ModuleName, checked: &CheckedModule) -> TypedModuleApi {
    let mut api = TypedModuleApi::default();
    let prefix = module_name.as_string() + ".";

    for (name, ty) in &checked.func_types {
        if let Some(stripped) = name.strip_prefix(&prefix) {
            api.term_types.insert(
                SymbolId {
                    module: module_name.clone(),
                    name: stripped.to_string(),
                },
                ty.clone(),
            );
        }
    }

    for (name, kind) in &checked.tycon_kinds {
        if let Some(stripped) = name.strip_prefix(&prefix) {
            api.type_kinds.insert(
                SymbolId {
                    module: module_name.clone(),
                    name: stripped.to_string(),
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
                    name: stripped.to_string(),
                },
                ty.clone(),
            );
        }
    }

    api
}

pub fn gather_imported_type_info(
    imports: &[ImportedTypedApi],
) -> (
    HashMap<String, TypeExpr>,
    HashMap<String, Kind>,
    HashMap<String, TypeExpr>,
) {
    let mut func_types = HashMap::new();
    let mut tycon_kinds = HashMap::new();
    let mut constructor_types = HashMap::new();

    for imp in imports {
        let filtered = match &imp.spec {
            ImportSpec::All => imp.api.as_ref().clone(),
            ImportSpec::Only(names) => {
                let mut out = TypedModuleApi::default();

                for name in names {
                    for (sym, ty) in &imp.api.term_types {
                        if &sym.name == name {
                            out.term_types.insert(sym.clone(), ty.clone());
                        }
                    }
                    for (sym, kind) in &imp.api.type_kinds {
                        if &sym.name == name {
                            out.type_kinds.insert(sym.clone(), kind.clone());
                        }
                    }
                    for (sym, ty) in &imp.api.constructor_types {
                        if &sym.name == name {
                            out.constructor_types.insert(sym.clone(), ty.clone());
                        }
                    }
                }

                out
            }
        };

        for (sym, ty) in filtered.term_types {
            func_types.insert(sym.display(), ty);
        }
        for (sym, kind) in filtered.type_kinds {
            tycon_kinds.insert(sym.display(), kind);
        }
        for (sym, ty) in filtered.constructor_types {
            constructor_types.insert(sym.display(), ty);
        }
    }

    (func_types, tycon_kinds, constructor_types)
}
