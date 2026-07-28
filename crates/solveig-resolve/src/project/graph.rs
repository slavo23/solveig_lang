//! Project dependency graph construction and topological ordering.
//! The driver relies on this ordering before resolving and checking modules.

use super::*;

#[derive(Debug, Clone)]
pub struct ModuleContext {
    pub module: ModuleName,
    pub module_id: ModuleId,
    pub file: FileId,
    pub local_api: SharedModuleApi,
    pub imported_apis: Vec<ImportedApi>,
    pub builtins: Rc<BuiltinApi>,
}

#[derive(Debug, Clone)]
pub struct ModuleFile {
    pub name: ModuleName,
    pub path: PathBuf,
    pub source_hash: u64,
    pub imports: Vec<ImportDecl>,
    pub parsed: ParsedModule,
}

#[derive(Debug)]
pub struct Project {
    pub modules: FxHashMap<ModuleName, ModuleFile>,
    pub module_table: ModuleTable,
}

pub struct ParsedProject {
    pub modules: FxHashMap<ModuleName, ParsedModule>,
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
    pub indices: FxHashMap<ModuleName, NodeIndex>,
    pub import_spans: FxHashMap<(ModuleName, ModuleName), FileSpan>,
}

pub fn build_module_graph(
    session: &Session,
    project: &Project,
) -> Result<ModuleGraph, ResolveError> {
    let mut graph = DiGraph::<ModuleName, ()>::new();
    let mut indices = FxHashMap::default();
    let mut import_spans = FxHashMap::default();

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
