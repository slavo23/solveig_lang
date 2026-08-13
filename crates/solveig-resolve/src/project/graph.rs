//! Project dependency graph construction and topological ordering.
//! The driver relies on this ordering before resolving and checking modules.

use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct ModuleContext {
    pub module: ModuleName,
    pub module_id: ModuleId,
    pub file: FileId,
    pub local_api: SharedModuleApi,
    pub imported_apis: Vec<ImportedApi>,
    pub builtins: Arc<BuiltinApi>,
}

#[derive(Debug, Clone)]
pub struct ModuleFile {
    pub package: PackageId,
    pub name: ModuleName,
    pub path: PathBuf,
    pub source_hash: u64,
    pub imports: Vec<ImportDecl>,
    pub parsed: ParsedModule,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub modules: FxHashMap<ModuleName, ModuleFile>,
    pub module_table: ModuleTable,
    package_kinds: FxHashMap<PackageId, PackageKind>,
    implicit_modules: Vec<ModuleName>,
    entry_module: ModuleName,
}

impl Project {
    pub fn new(
        session: &Session,
        modules: FxHashMap<ModuleName, ModuleFile>,
        package_kinds: FxHashMap<PackageId, PackageKind>,
        implicit_modules: Vec<ModuleName>,
        entry_module: ModuleName,
    ) -> Self {
        let module_table = ModuleTable::new(
            session,
            modules
                .values()
                .map(|module| (module.name.clone(), module.package)),
        );
        Self {
            modules,
            module_table,
            package_kinds,
            implicit_modules,
            entry_module,
        }
    }

    pub fn package_kind(&self, package: PackageId) -> Option<PackageKind> {
        self.package_kinds.get(&package).copied()
    }

    pub fn module_package(&self, module: &ModuleName) -> Option<PackageId> {
        self.module_table.package_for_name(module)
    }

    pub fn implicit_modules(&self) -> &[ModuleName] {
        &self.implicit_modules
    }

    pub fn entry_module(&self) -> &ModuleName {
        &self.entry_module
    }
}

#[derive(Clone, Debug)]
pub struct ParsedProject {
    pub modules: FxHashMap<ModuleName, ParsedModule>,
}

impl ParsedProject {
    pub fn get(&self, session: &Session, module: &ModuleName) -> &ParsedModule {
        self.modules.get(module).unwrap_or_else(|| {
            solveig_base::ice!("missing parsed module {}", module.as_string(session))
        })
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

    let mut module_names = project.modules.keys().collect::<Vec<_>>();
    module_names.sort_by_key(|module| module.as_string(session));

    for module_name in module_names {
        let ix = graph.add_node(module_name.clone());
        indices.insert(module_name.clone(), ix);
    }

    for (module_name, module_file) in &project.modules {
        let from = *indices
            .get(module_name)
            .expect("ICE: module graph missing node for known module");

        for imp in effective_imports(
            project,
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

        let ix = *scc
            .first()
            .unwrap_or_else(|| solveig_base::ice!("Tarjan returned an empty SCC"));
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
    session: &Session,
    graph: &ModuleGraph,
) -> Result<Vec<ModuleName>, ResolveError> {
    let node_count = graph.graph.node_count();
    let mut dependents = vec![Vec::<NodeIndex>::new(); node_count];
    let mut indegree = vec![0usize; node_count];

    for item in graph.graph.node_indices() {
        for dependency in graph.graph.neighbors(item) {
            dependents
                .get_mut(dependency.index())
                .expect("ICE: module graph dependency index out of bounds")
                .push(item);
            *indegree
                .get_mut(item.index())
                .expect("ICE: module graph item index out of bounds") += 1;
        }
    }

    for entries in &mut dependents {
        entries.sort_by_key(|node| graph.graph[*node].as_string(session));
    }

    let mut ready = graph
        .graph
        .node_indices()
        .filter(|node| {
            *indegree
                .get(node.index())
                .expect("ICE: module graph ready index out of bounds")
                == 0
        })
        .map(|node| (graph.graph[node].as_string(session), node.index()))
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(node_count);

    while let Some((_, node_index)) = ready.pop_first() {
        let node = NodeIndex::new(node_index);
        order.push(graph.graph[node].clone());

        let dependent_nodes = dependents
            .get(node_index)
            .expect("ICE: module graph dependent list index out of bounds")
            .clone();
        for dependent in dependent_nodes {
            let dependent_indegree = indegree
                .get_mut(dependent.index())
                .expect("ICE: module graph dependent indegree index out of bounds");
            *dependent_indegree -= 1;
            if *dependent_indegree == 0 {
                ready.insert((graph.graph[dependent].as_string(session), dependent.index()));
            }
        }
    }

    if order.len() != node_count {
        let cycle_node = graph
            .graph
            .node_indices()
            .find(|node| {
                *indegree
                    .get(node.index())
                    .expect("ICE: module graph cycle index out of bounds")
                    > 0
            })
            .expect("ICE: cyclic module graph must contain an unsorted node");
        let module = graph.graph[cycle_node].clone();
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
            .expect("ICE: module-order cycle diagnostic requires an import edge span");

        return Err(ResolveError::CyclicImports {
            span,
            modules: vec![module],
        });
    }

    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(session: &Session, name: &str) -> ModuleName {
        ModuleName(name.split('.').map(|part| session.intern(part)).collect())
    }

    fn graph_with_edges(
        session: &Session,
        modules: &[&str],
        edges: &[(&str, &str)],
    ) -> ModuleGraph {
        let mut graph = DiGraph::<ModuleName, ()>::new();
        let mut indices = FxHashMap::default();

        for name in modules {
            let module_name = module(session, name);
            let index = graph.add_node(module_name.clone());
            indices.insert(module_name, index);
        }

        for (from, to) in edges {
            let from = *indices
                .get(&module(session, from))
                .expect("test graph must contain source module");
            let to = *indices
                .get(&module(session, to))
                .expect("test graph must contain dependency module");
            graph.add_edge(from, to, ());
        }

        ModuleGraph {
            graph,
            indices,
            import_spans: FxHashMap::default(),
        }
    }

    #[test]
    fn module_order_is_dependency_first_with_name_tie_breaking() {
        let session = Session::new();
        let graph = graph_with_edges(
            &session,
            &["main", "d", "b", "a", "c"],
            &[("main", "a"), ("main", "b"), ("a", "c"), ("b", "c")],
        );

        let rendered = module_order(&session, &graph)
            .expect("test graph is acyclic")
            .into_iter()
            .map(|module| module.as_string(&session))
            .collect::<Vec<_>>();

        assert_eq!(rendered, vec!["c", "a", "b", "d", "main"]);
    }
}
