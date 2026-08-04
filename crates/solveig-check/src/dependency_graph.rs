//! Small graph helpers shared by checker passes.

use std::collections::BTreeSet;

use petgraph::algo::tarjan_scc;
use petgraph::graph::DiGraph;

fn first_component_node(components: &[Vec<usize>], component: usize) -> usize {
    *components
        .get(component)
        .and_then(|nodes| nodes.first())
        .expect("ICE: SCC component index must refer to a non-empty component")
}

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
            let item_node = *nodes.get(item).expect("ICE: SCC item index out of bounds");
            graph.add_edge(item_node, dependency_node, ());
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
            *component_by_node
                .get_mut(node)
                .expect("ICE: SCC node index out of bounds") = component_id;
        }
        components.push(component);
    }

    let mut component_edges = BTreeSet::new();
    let mut dependents = vec![Vec::new(); components.len()];
    let mut indegree = vec![0; components.len()];

    for (item, dependencies) in adjacency.iter().enumerate() {
        let item_component = *component_by_node
            .get(item)
            .expect("ICE: SCC item component index out of bounds");
        for &dependency in dependencies {
            let dependency_component = *component_by_node
                .get(dependency)
                .expect("ICE: SCC dependency component index out of bounds");
            if dependency_component == item_component {
                continue;
            }

            if component_edges.insert((dependency_component, item_component)) {
                dependents
                    .get_mut(dependency_component)
                    .expect("ICE: SCC dependency component out of bounds")
                    .push(item_component);
                *indegree
                    .get_mut(item_component)
                    .expect("ICE: SCC item component out of bounds") += 1;
            }
        }
    }

    for entries in &mut dependents {
        entries.sort_by_key(|component| first_component_node(&components, *component));
    }

    let mut ready = components
        .iter()
        .enumerate()
        .filter(|(component, _)| {
            *indegree
                .get(*component)
                .expect("ICE: SCC ready component out of bounds")
                == 0
        })
        .map(|(component, nodes)| {
            (
                *nodes.first().expect("ICE: SCC component must not be empty"),
                component,
            )
        })
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::with_capacity(components.len());

    while let Some((_, component)) = ready.pop_first() {
        ordered.push(
            components
                .get(component)
                .expect("ICE: SCC ordered component out of bounds")
                .clone(),
        );

        let dependent_components = dependents
            .get(component)
            .expect("ICE: SCC dependent list out of bounds")
            .clone();
        for dependent in dependent_components {
            let dependent_indegree = indegree
                .get_mut(dependent)
                .expect("ICE: SCC dependent indegree out of bounds");
            *dependent_indegree -= 1;
            if *dependent_indegree == 0 {
                ready.insert((first_component_node(&components, dependent), dependent));
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
