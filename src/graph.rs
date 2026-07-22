//! Run graph — a petgraph DAG over planned tasks.
//!
//! Nodes are canonical task ids (`namespace:name`); edges are `needs`
//! prerequisites. `build` validates that every dependency resolves and rejects
//! cycles. `run_order` is a topological sort (the sequential execution order);
//! `layers` groups tasks by dependency depth (Kahn) — each layer is
//! parallel-safe, for the future concurrent executor.

use crate::error::HtError;
use crate::error::Result;
use crate::plan::PlannedTask;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// The run graph: a DAG of tasks ordered by their `needs` edges. Nodes are
/// canonical task ids (`namespace:name`).
pub struct RunGraph {
    graph: DiGraph<String, ()>,
    index: HashMap<String, NodeIndex>,
}

impl RunGraph {
    /// Build from the assembled plan. Ids/deps are already validated as unique
    /// and resolvable by `plan::assemble`; this adds cycle detection.
    pub fn build(planned: &[PlannedTask]) -> Result<Self> {
        let mut graph = DiGraph::<String, ()>::new();
        let mut index = HashMap::new();

        for p in planned {
            let idx = graph.add_node(p.id.clone());
            index.insert(p.id.clone(), idx);
        }

        // Edge: dependency -> dependent (points in execution direction).
        for p in planned {
            let to = index[&p.id];
            for dep in &p.needs {
                let from = *index.get(dep).ok_or_else(|| {
                    HtError::UnknownDep(p.id.clone(), dep.clone())
                })?;
                graph.add_edge(from, to, ());
            }
        }

        let rg = RunGraph { graph, index };
        rg.detect_cycle()?;
        Ok(rg)
    }

    fn detect_cycle(&self) -> Result<()> {
        if let Err(cycle) = petgraph::algo::toposort(&self.graph, None) {
            let name = self.graph[cycle.node_id()].clone();
            return Err(HtError::Cycle(name).into());
        }
        Ok(())
    }

    /// Kahn's algorithm grouped by depth. Each inner Vec is a set of tasks
    /// with no remaining unmet deps — safe to run concurrently.
    pub fn layers(&self) -> Vec<Vec<String>> {
        let mut indeg: BTreeMap<NodeIndex, usize> = self
            .graph
            .node_indices()
            .map(|n| {
                (
                    n,
                    self.graph
                        .neighbors_directed(n, Direction::Incoming)
                        .count(),
                )
            })
            .collect();

        let mut queue: VecDeque<NodeIndex> = indeg
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(&n, _)| n)
            .collect();

        let mut layers = Vec::new();
        let mut done = 0usize;

        while !queue.is_empty() {
            // Sort each layer by name for deterministic output.
            let mut layer: Vec<NodeIndex> = queue.drain(..).collect();
            layer.sort_by_key(|n| self.graph[*n].clone());

            let mut names = Vec::with_capacity(layer.len());
            for n in &layer {
                names.push(self.graph[*n].clone());
                done += 1;
                for succ in self.graph.neighbors_directed(*n, Direction::Outgoing)
                {
                    let d = indeg.get_mut(&succ).unwrap();
                    *d -= 1;
                    if *d == 0 {
                        queue.push_back(succ);
                    }
                }
            }
            layers.push(names);
        }

        debug_assert_eq!(done, self.graph.node_count());
        layers
    }

    /// Flat execution order (topological). Used by the sequential engine.
    pub fn run_order(&self) -> Vec<String> {
        self.layers().into_iter().flatten().collect()
    }

    #[allow(dead_code)]
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;
    use crate::plan::{self, LoadedFile};

    fn file(ns: &str, yaml: &str) -> LoadedFile {
        let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
        LoadedFile {
            namespace: ns.to_string(),
            manifest,
            loop_groups: Default::default(),
        }
    }

    #[test]
    fn layers_respect_deps() {
        let files = [file(
            "f",
            "tasks:\n\
             - {name: a}\n\
             - {name: b, needs: [a]}\n\
             - {name: c, needs: [a]}\n\
             - {name: d, needs: [b, c]}\n",
        )];
        let planned = plan::assemble(&files).unwrap();
        let rg = RunGraph::build(&planned).unwrap();
        assert_eq!(
            rg.layers(),
            vec![vec!["f:a"], vec!["f:b", "f:c"], vec!["f:d"]]
        );
    }

    #[test]
    fn cross_file_dep_orders_correctly() {
        let files = [
            file("base", "tasks:\n- {name: login}\n"),
            file("users", "tasks:\n- {name: create, needs: [base:login]}\n"),
        ];
        let planned = plan::assemble(&files).unwrap();
        let rg = RunGraph::build(&planned).unwrap();
        assert_eq!(rg.layers(), vec![vec!["base:login"], vec!["users:create"]]);
    }

    #[test]
    fn cycle_is_rejected() {
        let files = [file(
            "f",
            "tasks:\n\
             - {name: a, needs: [b]}\n\
             - {name: b, needs: [a]}\n",
        )];
        let planned = plan::assemble(&files).unwrap();
        assert!(RunGraph::build(&planned).is_err());
    }

    #[test]
    fn unknown_dep_is_rejected() {
        let files = [file("f", "tasks:\n- {name: a, needs: [ghost]}\n")];
        assert!(plan::assemble(&files).is_err());
    }

    #[test]
    fn duplicate_id_is_rejected() {
        let files = [file("f", "tasks:\n- {name: a}\n- {name: a}\n")];
        assert!(plan::assemble(&files).is_err());
    }
}
