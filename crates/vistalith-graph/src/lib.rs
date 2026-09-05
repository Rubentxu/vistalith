//! In-memory Semantic World Graph (SPEC-001) with event-sourced projection
//! (SPEC-002), graph patches with optimistic concurrency (SPEC-004) and
//! deterministic fixture replay (`roadmap/IMPLEMENT-NOW.md` items 5-6).
//!
//! The graph is a materialized view: durable sources are the `VEvent` log;
//! the whole graph is reconstructible from it. Storage is intentionally
//! simple maps (`IMPLEMENT-NOW.md`: "in-memory SWG with petgraph or simple
//! maps"); graph algorithms arrive with ADR-007 once queries need them.

mod algorithms;
mod behavior;
mod c4;
mod context;
mod decisions;
mod diff;
mod why;
mod digest;
mod graph;
mod patch;
mod projection;
mod store;

pub use algorithms::{
    AlgorithmGraph, CycleReport, ImpactAnalysis, ImpactReport, PathReport, TopoReport,
};
pub use decisions::{DecisionAlternative, DecisionEntry, DecisionsLens, decisions_lens};
pub use why::{WhyLink, WhyPath, why_path};
pub use context::{
    ContextExclusion, ContextItem, ContextRequest, ExclusionReason, InclusionReason,
    SemanticContextView, build_context_view,
};
pub use behavior::{
    Behavior, BehaviorContext, BehaviorError, BehaviorSpec, ContradictionAdvisory,
    ImpactAdvisory, MissingEvidenceAdvisory, StaleEvidenceAdvisory, append_and_react,
    builtin_behaviors,
};
pub use c4::{
    C4Element, C4Level, C4Relationship, C4View, c4_view, is_c4_subject, is_structural_relation,
};
pub use diff::{
    GraphDiff, PropertyChange, RelationChange, SubjectChange, diff_graphs,
};
pub use digest::{canonical_graph_json, graph_digest};
pub use graph::{SemanticWorldGraph, SubjectNode};
pub use patch::{GraphPatch, PatchOutcome, RejectionReason};
pub use projection::{ProjectionError, apply_event};
pub use store::{AppendedEvent, GraphStore, StoreError};
