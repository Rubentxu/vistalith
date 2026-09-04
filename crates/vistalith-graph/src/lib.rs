//! In-memory Semantic World Graph (SPEC-001) with event-sourced projection
//! (SPEC-002), graph patches with optimistic concurrency (SPEC-004) and
//! deterministic fixture replay (`roadmap/IMPLEMENT-NOW.md` items 5-6).
//!
//! The graph is a materialized view: durable sources are the `VEvent` log;
//! the whole graph is reconstructible from it. Storage is intentionally
//! simple maps (`IMPLEMENT-NOW.md`: "in-memory SWG with petgraph or simple
//! maps"); graph algorithms arrive with ADR-007 once queries need them.

mod digest;
mod graph;
mod patch;
mod projection;
mod store;

pub use digest::{canonical_graph_json, graph_digest};
pub use graph::{SemanticWorldGraph, SubjectNode};
pub use patch::{GraphPatch, PatchOutcome, RejectionReason};
pub use projection::{ProjectionError, apply_event};
pub use store::{AppendedEvent, GraphStore, StoreError};
