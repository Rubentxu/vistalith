use std::fmt;

use serde::{Deserialize, Serialize};
use vistalith_domain::{Actor, Namespace, PatchId, PatchOperation, SubjectRef};

use crate::graph::SemanticWorldGraph;

/// A proposed mutation of Vistalith-owned graph state (SPEC-004).
///
/// Lifecycle: `proposed → applied | rejected`. Carries the base graph
/// revision as its optimistic concurrency token; rejected patches become
/// durable events (SPEC-002) and never mutate the graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphPatch {
    pub patch_id: PatchId,
    /// Graph revision the proposal was made against; a mismatch rejects the
    /// whole patch (`StaleBase`).
    pub base_revision: u64,
    pub proposed_by: Actor,
    pub operations: Vec<PatchOperation>,
}

/// Typed reason a patch was rejected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reason", content = "detail", rename_all = "kebab-case")]
pub enum RejectionReason {
    /// The graph moved on since the patch was proposed.
    StaleBase { expected: u64, got: u64 },
    /// The patch would authoritatively mutate an SDDK-owned subject. It must
    /// be converted into a governed SDDK semantic proposal instead
    /// (SPEC-001 invariant 4 / SPEC-004).
    MustBeGovernedBySddk { subject: SubjectRef },
    /// The patch references a subject that does not exist.
    UnknownSubject { subject: SubjectRef },
    /// Anything else that makes the patch unappliable.
    InvalidOperation(String),
}

impl fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RejectionReason::StaleBase { expected, got } => {
                write!(
                    f,
                    "stale base revision: patch proposed at {got}, graph is at {expected}"
                )
            }
            RejectionReason::MustBeGovernedBySddk { subject } => {
                write!(
                    f,
                    "subject `{subject}` is SDDK-owned: convert this change into a governed SDDK semantic proposal"
                )
            }
            RejectionReason::UnknownSubject { subject } => {
                write!(f, "unknown subject `{subject}`")
            }
            RejectionReason::InvalidOperation(detail) => {
                write!(f, "invalid patch operation: {detail}")
            }
        }
    }
}

/// Result of proposing a patch. Rejections are still recorded as durable
/// `patch-rejected` events.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum PatchOutcome {
    Applied { patch_id: PatchId, revision: u64 },
    Rejected { patch_id: PatchId, reason: String },
}

/// Validates a patch against the graph. Pure: no mutation.
pub fn validate_patch(
    graph: &SemanticWorldGraph,
    patch: &GraphPatch,
) -> Result<(), RejectionReason> {
    if patch.base_revision != graph.revision() {
        return Err(RejectionReason::StaleBase {
            expected: graph.revision(),
            got: patch.base_revision,
        });
    }

    for op in &patch.operations {
        match op {
            PatchOperation::UpsertSubject {
                subject, authority, ..
            } => {
                if graph
                    .node(subject)
                    .is_some_and(crate::graph::SubjectNode::is_sddk_owned)
                    || (subject.namespace() == &Namespace::Sddk && authority.is_authoritative())
                {
                    return Err(RejectionReason::MustBeGovernedBySddk {
                        subject: subject.clone(),
                    });
                }
            }
            PatchOperation::DeclareRelation { fact } => {
                if !graph.relation_endpoint_exists(&fact.relation) {
                    let missing = if graph.node(&fact.relation.from).is_none() {
                        fact.relation.from.clone()
                    } else {
                        fact.relation.to.clone()
                    };
                    return Err(RejectionReason::UnknownSubject { subject: missing });
                }
                // No Vistalith patch may attach an authoritative fact to an
                // SDDK-namespace subject: even when Vistalith only holds an
                // observation (derived), authority over its facts stays with
                // SDDK (SPEC-001 invariant 4).
                if fact.authority.is_authoritative() {
                    let endpoint = [&fact.relation.from, &fact.relation.to]
                        .into_iter()
                        .find(|s| {
                            s.namespace() == &Namespace::Sddk
                                || graph
                                    .node(s)
                                    .is_some_and(crate::graph::SubjectNode::is_sddk_owned)
                        });
                    if let Some(subject) = endpoint {
                        return Err(RejectionReason::MustBeGovernedBySddk {
                            subject: subject.clone(),
                        });
                    }
                }
            }
            PatchOperation::DeprecateSubject { subject, .. } => match graph.node(subject) {
                None => {
                    return Err(RejectionReason::UnknownSubject {
                        subject: subject.clone(),
                    });
                }
                Some(node) if node.is_sddk_owned() => {
                    return Err(RejectionReason::MustBeGovernedBySddk {
                        subject: subject.clone(),
                    });
                }
                Some(_) => {}
            },
        }
    }
    Ok(())
}
