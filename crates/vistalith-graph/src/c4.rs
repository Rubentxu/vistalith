//! C4 projection (IMPLEMENT-NOW.md item 12: "one C4 projection").
//!
//! The C4 view is a *projection*: it is derived from the SWG on demand and
//! carries no state of its own. Elements are subjects whose kind belongs to
//! the architecture family (`system`, `container`, `component`); relationships
//! are graph relations whose endpoints are both C4 elements. Everything is
//! canonically ordered, so the same graph always yields the same view.

use serde::Serialize;
use vistalith_domain::{RelationKind, SubjectKind, SubjectRef};

use crate::graph::SemanticWorldGraph;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum C4Level {
    System,
    Container,
    Component,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct C4Element {
    /// Stable SubjectRef identity — the C4 lens maps renderer ids to this.
    pub identity: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub authority: vistalith_domain::AuthorityClass,
    pub deprecated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct C4Relationship {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub authority: vistalith_domain::AuthorityClass,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct C4View {
    pub revision: u64,
    pub systems: Vec<C4Element>,
    pub containers: Vec<C4Element>,
    pub components: Vec<C4Element>,
    pub relationships: Vec<C4Relationship>,
}

impl C4View {
    pub fn all_elements(&self) -> impl Iterator<Item = &C4Element> {
        self.systems
            .iter()
            .chain(self.containers.iter())
            .chain(self.components.iter())
    }
}

/// Projects the graph into a C4 view (context + containers + components).
pub fn c4_view(graph: &SemanticWorldGraph) -> C4View {
    let mut systems = Vec::new();
    let mut containers = Vec::new();
    let mut components = Vec::new();
    let mut identities = std::collections::HashSet::new();

    for node in graph.subjects() {
        let (level, bucket) = match c4_level(node.subject.kind()) {
            Some(level) => match level {
                C4Level::System => (level, &mut systems),
                C4Level::Container => (level, &mut containers),
                C4Level::Component => (level, &mut components),
            },
            None => continue,
        };
        let _ = level;
        identities.insert(node.subject.to_string());
        bucket.push(C4Element {
            identity: node.subject.to_string(),
            name: element_name(node),
            description: node
                .properties
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
            authority: node.authority,
            deprecated: node.deprecated,
        });
    }

    let relationships = graph
        .relations()
        .filter(|fact| {
            identities.contains(&fact.relation.from.to_string())
                && identities.contains(&fact.relation.to.to_string())
        })
        .map(|fact| C4Relationship {
            source: fact.relation.from.to_string(),
            target: fact.relation.to.to_string(),
            kind: fact.relation.kind.to_string(),
            authority: fact.authority,
        })
        .collect();

    C4View {
        revision: graph.revision(),
        systems,
        containers,
        components,
        relationships,
    }
}

fn c4_level(kind: &SubjectKind) -> Option<C4Level> {
    match kind {
        SubjectKind::System => Some(C4Level::System),
        SubjectKind::Container => Some(C4Level::Container),
        SubjectKind::Component => Some(C4Level::Component),
        _ => None,
    }
}

fn element_name(node: &crate::graph::SubjectNode) -> String {
    node.properties
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| node.subject.id().to_owned())
}

/// Convenience: subjects that are C4 elements, for lens navigation.
pub fn is_c4_subject(subject: &SubjectRef) -> bool {
    c4_level(subject.kind()).is_some()
}

/// Re-exported for consumers that need the relation kinds C4 draws as-is.
pub fn is_structural_relation(kind: &RelationKind) -> bool {
    matches!(
        kind,
        RelationKind::DependsOn
            | RelationKind::Implements
            | RelationKind::Calls
            | RelationKind::Exposes
            | RelationKind::Contains
    )
}
