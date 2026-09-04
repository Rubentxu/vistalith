use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use vistalith_domain::RelationRef;

use crate::graph::{SemanticWorldGraph, SubjectNode};

#[derive(Serialize)]
struct GraphDto<'a> {
    revision: u64,
    subjects: Vec<&'a SubjectNode>,
    relations: Vec<RelationFactDto<'a>>,
}

#[derive(Serialize)]
struct RelationFactDto<'a> {
    relation: &'a RelationRef,
    authority: &'a vistalith_domain::AuthorityClass,
    provenance: &'a vistalith_domain::Provenance,
}

/// Canonical JSON of the graph: iteration order comes from the ordered
/// containers inside [`SemanticWorldGraph`], so the output is byte-stable
/// across processes and runs — the basis of the digest.
pub fn canonical_graph_json(graph: &SemanticWorldGraph) -> String {
    let dto = GraphDto {
        revision: graph.revision(),
        subjects: graph.subjects().collect(),
        relations: graph
            .relations()
            .map(|fact| RelationFactDto {
                relation: &fact.relation,
                authority: &fact.authority,
                provenance: &fact.provenance,
            })
            .collect(),
    };
    serde_json::to_string(&dto).expect("graph DTO serialization cannot fail")
}

/// Stable SHA-256 fingerprint of the graph state. Two replays of the same
/// log always produce the same digest; that equality is the determinism
/// gate for fixture replay and rebuilds.
pub fn graph_digest(graph: &SemanticWorldGraph) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_graph_json(graph).as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Deserialize)]
pub(crate) struct RawLog {
    #[serde(default)]
    pub events: Vec<vistalith_domain::VEvent>,
}

#[derive(Deserialize)]
pub(crate) struct StoredLog {
    #[serde(default)]
    pub events: Vec<vistalith_domain::StoredEvent>,
}

#[derive(Serialize)]
pub(crate) struct StoredLogOut<'a> {
    pub events: &'a [vistalith_domain::StoredEvent],
}
