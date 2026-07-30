//! Small graph helpers shared by checker passes.

use std::collections::BTreeSet;

use petgraph::algo::tarjan_scc;
use petgraph::graph::DiGraph;

/// Returns SCCs for `item -> dependency` edges in dependency-first order.
pub(crate) fn dependency_ordered_sccs(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    if adjacency.is_empty() {
        return Vec::new();
    }

    let mut graph = DiGraph::<(), ()>::new();
    let nodes = (0..adjacency.len())
        .map(|_| graph.add_node(()))
        .collect::<Vec<_>>();

    for (item, dependencies) in adjacency.iter().enumerate() {
        for &dependency in dependencies {
            let dependency_node = *nodes
                .get(dependency)
                .expect("ICE: SCC dependency index out of bounds");
            graph.add_edge(nodes[item], dependency_node, ());
        }
    }

    let mut components = Vec::new();
    let mut component_by_node = vec![usize::MAX; adjacency.len()];

    for raw_component in tarjan_scc(&graph) {
        let mut component = raw_component
            .into_iter()
            .map(|node| node.index())
            .collect::<Vec<_>>();
        component.sort_unstable();

        let component_id = components.len();
        for &node in &component {
            component_by_node[node] = component_id;
        }
        components.push(component);
    }

    let mut component_edges = BTreeSet::new();
    let mut dependents = vec![Vec::new(); components.len()];
    let mut indegree = vec![0; components.len()];

    for (item, dependencies) in adjacency.iter().enumerate() {
        let item_component = component_by_node[item];
        for &dependency in dependencies {
            let dependency_component = component_by_node[dependency];
            if dependency_component == item_component {
                continue;
            }

            if component_edges.insert((dependency_component, item_component)) {
                dependents[dependency_component].push(item_component);
                indegree[item_component] += 1;
            }
        }
    }

    for entries in &mut dependents {
        entries.sort_by_key(|component| components[*component][0]);
    }

    let mut ready = components
        .iter()
        .enumerate()
        .filter(|(component, _)| indegree[*component] == 0)
        .map(|(component, nodes)| (nodes[0], component))
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::with_capacity(components.len());

    while let Some((_, component)) = ready.pop_first() {
        ordered.push(components[component].clone());

        for &dependent in &dependents[component] {
            indegree[dependent] -= 1;
            if indegree[dependent] == 0 {
                ready.insert((components[dependent][0], dependent));
            }
        }
    }

    debug_assert_eq!(
        ordered.len(),
        components.len(),
        "ICE: SCC condensation graph must be acyclic"
    );
    ordered
}

#[cfg(test)]
mod tests {
    use super::dependency_ordered_sccs;

    #[test]
    fn orders_dependencies_before_dependents() {
        assert_eq!(
            dependency_ordered_sccs(&[vec![1], vec![]]),
            vec![vec![1], vec![0],]
        );
    }

    #[test]
    fn groups_cycles_and_keeps_independent_source_order() {
        assert_eq!(
            dependency_ordered_sccs(&[vec![1], vec![0], vec![]]),
            vec![vec![0, 1], vec![2]]
        );
    }
}
