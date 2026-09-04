//! Algorithmic graph operations on an extracted snapshot of the SWG
//! (ADR-007: petgraph for algorithms, independent of persistence and of any
//! query language; SPK-004: traversal / impact / SCC / path).
//!
//! The SWG itself stays plain ordered maps — the projection is the truth —
//! but algorithmic questions (what is impacted by this change? is there a
//! dependency cycle? what connects A to B?) are answered by converting to a
//! `petgraph::DiGraph` on demand. The conversion is deterministic (ordered
//! containers), so results are stable across runs.

use std::collections::HashMap;

use petgraph::algo::{kosaraju_scc, toposort};
use petgraph::graph::NodeIndex;
use petgraph::visit::{Bfs, Dfs, Reversed};
use petgraph::graph::DiGraph;
use serde::Serialize;
use vistalith_domain::{RelationKind, SubjectRef};

use crate::graph::SemanticWorldGraph;

/// An extracted algorithmic snapshot: subject identities in one map, edges
/// labeled with their relation kind.
pub struct AlgorithmGraph {
    graph: DiGraph<SubjectRef, RelationKind>,
    index: HashMap<SubjectRef, NodeIndex>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactReport {
    pub root: String,
    /// Subjects transitively impacted by a change to `root` (everything that
    /// depends on it, directly or not), ordered by identity.
    pub impacted: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathReport {
    pub from: String,
    pub to: String,
    /// Subject identities along the path, inclusive of both ends.
    pub path: Vec<String>,
    pub relation_kinds: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CycleReport {
    /// Strongly connected components that are actual cycles (more than one
    /// member, or a self-loop), ordered by their smallest identity.
    pub cycles: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopoReport {
    pub ordered: Vec<String>,
    /// True when the snapshot had a cycle and no topological order exists.
    pub cyclic: bool,
}

impl AlgorithmGraph {
    /// Extracts a snapshot following (optionally) only the given relation
    /// kinds; `None` follows every edge. Deprecated subjects are included:
    /// impact questions must see the whole structure.
    pub fn extract(graph: &SemanticWorldGraph, kinds: Option<&[RelationKind]>) -> Self {
        let mut digraph = DiGraph::new();
        let mut index = HashMap::new();
        for node in graph.subjects() {
            let node_index = digraph.add_node(node.subject.clone());
            index.insert(node.subject.clone(), node_index);
        }
        for fact in graph.relations() {
            if let Some(kinds) = kinds
                && !kinds.contains(&fact.relation.kind)
            {
                continue;
            }
            let (Some(from), Some(to)) = (
                index.get(&fact.relation.from),
                index.get(&fact.relation.to),
            ) else {
                continue;
            };
            digraph.add_edge(*from, *to, fact.relation.kind.clone());
        }
        AlgorithmGraph {
            graph: digraph,
            index,
        }
    }

    /// Transitive dependents of `root`: everything whose fate passes through
    /// it, following edges backwards (X depends_on root means X is impacted).
    pub fn impact_of(&self, root: &SubjectRef) -> Option<ImpactReport> {
        let start = *self.index.get(root)?;
        let reversed = Reversed(&self.graph);
        let mut dfs = Dfs::new(reversed, start);
        let mut impacted = Vec::new();
        while let Some(node) = dfs.next(reversed) {
            if node != start {
                impacted.push(self.graph[node].to_string());
            }
        }
        impacted.sort();
        Some(ImpactReport {
            root: root.to_string(),
            impacted,
        })
    }

    /// Shortest path (fewest hops, BFS) from `from` to `to`, following edge
    /// direction. Inclusive of both ends.
    pub fn shortest_path(&self, from: &SubjectRef, to: &SubjectRef) -> Option<PathReport> {
        let start = *self.index.get(from)?;
        let goal = *self.index.get(to)?;
        if start == goal {
            return Some(PathReport {
                from: from.to_string(),
                to: to.to_string(),
                path: vec![from.to_string()],
                relation_kinds: Vec::new(),
            });
        }
        let mut parents: HashMap<NodeIndex, NodeIndex> = HashMap::new();
        let mut bfs = Bfs::new(&self.graph, start);
        let mut found = false;
        while let Some(node) = bfs.next(&self.graph) {
            if node == goal {
                found = true;
                break;
            }
            for neighbor in self
                .graph
                .neighbors_directed(node, petgraph::Direction::Outgoing)
            {
                parents.entry(neighbor).or_insert(node);
            }
        }
        if !found || !parents.contains_key(&goal) {
            return None;
        }
        let mut chain = vec![goal];
        while let Some(&parent) = parents.get(chain.last().unwrap()) {
            chain.push(parent);
            if parent == start {
                break;
            }
        }
        chain.reverse();
        let path: Vec<String> = chain
            .iter()
            .map(|node| self.graph[*node].to_string())
            .collect();
        let relation_kinds: Vec<String> = chain
            .windows(2)
            .filter_map(|pair| {
                self.graph
                    .find_edge(pair[0], pair[1])
                    .map(|edge| self.graph[edge].as_str().to_owned())
            })
            .collect();
        Some(PathReport {
            from: from.to_string(),
            to: to.to_string(),
            path,
            relation_kinds,
        })
    }

    /// Strongly connected components that are real cycles.
    pub fn cycles(&self) -> CycleReport {
        let mut cycles: Vec<Vec<String>> = kosaraju_scc(&self.graph)
            .into_iter()
            .filter(|scc| {
                scc.len() > 1
                    || (scc.len() == 1 && self.graph.find_edge(scc[0], scc[0]).is_some())
            })
            .map(|scc| {
                let mut ids: Vec<String> = scc
                    .iter()
                    .map(|node| self.graph[*node].to_string())
                    .collect();
                ids.sort();
                ids
            })
            .collect();
        cycles.sort();
        CycleReport { cycles }
    }

    /// Topological order over the snapshot; `cyclic` when impossible.
    pub fn topological_order(&self) -> TopoReport {
        match toposort(&self.graph, None) {
            Ok(order) => TopoReport {
                ordered: order
                    .into_iter()
                    .map(|node| self.graph[node].to_string())
                    .collect(),
                cyclic: false,
            },
            Err(_) => TopoReport {
                ordered: Vec::new(),
                cyclic: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use vistalith_domain::{
        AuthorityClass, Namespace, Provenance, RelationFact, RelationRef, SubjectKind,
    };

    fn container(id: &str) -> SubjectRef {
        SubjectRef::new(Namespace::Arch, SubjectKind::Container, id).unwrap()
    }

    fn graph_with_chain() -> SemanticWorldGraph {
        // gateway -> payment-service -> ledger -> database
        let mut graph = SemanticWorldGraph::new();
        let provenance = Provenance::new("test:algorithms").unwrap();
        for id in ["gateway", "payment-service", "ledger", "database"] {
            graph.upsert_subject(
                container(id),
                AuthorityClass::Authoritative,
                provenance.clone(),
                BTreeMap::new(),
                0,
            );
        }
        for (from, to) in [
            ("gateway", "payment-service"),
            ("payment-service", "ledger"),
            ("ledger", "database"),
        ] {
            graph.declare_relation(
                RelationFact {
                    relation: RelationRef::new(
                        container(from),
                        RelationKind::DependsOn,
                        container(to),
                    )
                    .unwrap(),
                    authority: AuthorityClass::Authoritative,
                    provenance: provenance.clone(),
                },
                0,
            );
        }
        graph
    }

    #[test]
    fn impact_follows_dependents_transitively() {
        let snapshot = AlgorithmGraph::extract(&graph_with_chain(), None);
        let report = snapshot.impact_of(&container("database")).unwrap();
        assert_eq!(
            report.impacted,
            vec![
                "arch:container:gateway",
                "arch:container:ledger",
                "arch:container:payment-service"
            ]
        );
        let leaf = snapshot.impact_of(&container("gateway")).unwrap();
        assert!(leaf.impacted.is_empty(), "nothing depends on gateway");
    }

    #[test]
    fn shortest_path_walks_edge_direction() {
        let snapshot = AlgorithmGraph::extract(&graph_with_chain(), None);
        let report = snapshot
            .shortest_path(&container("gateway"), &container("database"))
            .unwrap();
        assert_eq!(report.path.len(), 4);
        assert_eq!(report.relation_kinds, vec!["depends_on"; 3]);
        assert!(
            snapshot
                .shortest_path(&container("database"), &container("gateway"))
                .is_none(),
            "edges are directed"
        );
    }

    #[test]
    fn cycles_reports_strongly_connected_components() {
        let mut graph = graph_with_chain();
        let provenance = Provenance::new("test:algorithms").unwrap();
        graph.declare_relation(
            RelationFact {
                relation: RelationRef::new(
                    container("database"),
                    RelationKind::DependsOn,
                    container("gateway"),
                )
                .unwrap(),
                authority: AuthorityClass::Authoritative,
                provenance,
            },
            0,
        );
        let snapshot = AlgorithmGraph::extract(&graph, None);
        let report = snapshot.cycles();
        assert_eq!(report.cycles.len(), 1);
        assert_eq!(report.cycles[0].len(), 4);
        assert!(snapshot.topological_order().cyclic);
    }

    #[test]
    fn extraction_can_restrict_edge_kinds() {
        let mut graph = graph_with_chain();
        let provenance = Provenance::new("test:algorithms").unwrap();
        graph.declare_relation(
            RelationFact {
                relation: RelationRef::new(
                    container("gateway"),
                    RelationKind::Mentions,
                    container("database"),
                )
                .unwrap(),
                authority: AuthorityClass::Advisory,
                provenance,
            },
            0,
        );
        let depends_only = AlgorithmGraph::extract(&graph, Some(&[RelationKind::DependsOn]));
        assert!(
            depends_only
                .shortest_path(&container("gateway"), &container("database"))
                .is_some()
        );
        let none = AlgorithmGraph::extract(&graph, Some(&[]));
        assert!(
            none.shortest_path(&container("gateway"), &container("database"))
                .is_none(),
            "empty allowlist extracts no edges"
        );
    }
}
