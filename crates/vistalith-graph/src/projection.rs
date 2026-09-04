use vistalith_domain::{EventPayload, PatchOperation, VEvent};

use crate::graph::SemanticWorldGraph;

/// Strict projection errors: replaying a valid log never fails; an error here
/// means the log itself is inconsistent (a fixture bug or corruption).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProjectionError {
    #[error("subject `{0}` already exists")]
    DuplicateSubject(String),
    #[error("unknown subject `{0}`")]
    UnknownSubject(String),
    #[error("relation `{0}` already exists")]
    DuplicateRelation(String),
    #[error("invalid patch operation: {0}")]
    InvalidOperation(String),
}

/// Applies one event to the graph. Returns the graph revision after the
/// event: state-changing events bump the revision; `patch-rejected` events
/// are durable but leave the graph (and revision) untouched.
pub fn apply_event(
    graph: &mut SemanticWorldGraph,
    event: &VEvent,
    sequence: u64,
) -> Result<u64, ProjectionError> {
    match &event.payload {
        EventPayload::SubjectDefined(defined) => {
            if graph.node(&defined.subject).is_some() {
                return Err(ProjectionError::DuplicateSubject(
                    defined.subject.to_string(),
                ));
            }
            graph.upsert_subject(
                defined.subject.clone(),
                defined.authority,
                defined.provenance.clone(),
                defined.properties.clone(),
                sequence,
            );
        }
        EventPayload::SubjectUpdated(updated) => {
            if !graph.update_subject(&updated.subject, &updated.properties, sequence) {
                return Err(ProjectionError::UnknownSubject(updated.subject.to_string()));
            }
        }
        EventPayload::SubjectDeprecated(deprecated) => {
            if !graph.deprecate_subject(&deprecated.subject, sequence) {
                return Err(ProjectionError::UnknownSubject(
                    deprecated.subject.to_string(),
                ));
            }
        }
        EventPayload::RelationDeclared(declared) => {
            let fact = &declared.fact;
            if !graph.relation_endpoint_exists(&fact.relation) {
                let missing = if graph.node(&fact.relation.from).is_none() {
                    fact.relation.from.to_string()
                } else {
                    fact.relation.to.to_string()
                };
                return Err(ProjectionError::UnknownSubject(missing));
            }
            if !graph.declare_relation(fact.clone(), sequence) {
                return Err(ProjectionError::DuplicateRelation(
                    fact.relation.to_string(),
                ));
            }
        }
        EventPayload::PatchApplied(applied) => {
            apply_operations(graph, &applied.operations, sequence)?;
        }
        EventPayload::PatchRejected(_) => {
            // Durable for auditability only; the graph does not change.
            return Ok(graph.revision());
        }
    }
    Ok(graph.bump_revision())
}

/// Applies patch operations. Callers validate first (`patch::validate_patch`);
/// during replay this is re-checked structurally so a corrupted log fails
/// loudly instead of producing a silently wrong graph.
pub fn apply_operations(
    graph: &mut SemanticWorldGraph,
    operations: &[PatchOperation],
    sequence: u64,
) -> Result<(), ProjectionError> {
    for op in operations {
        match op {
            PatchOperation::UpsertSubject {
                subject,
                authority,
                provenance,
                properties,
            } => {
                if graph
                    .node(subject)
                    .is_some_and(crate::graph::SubjectNode::is_sddk_owned)
                {
                    return Err(ProjectionError::InvalidOperation(format!(
                        "upsert of SDDK-owned subject `{subject}`"
                    )));
                }
                graph.upsert_subject(
                    subject.clone(),
                    *authority,
                    provenance.clone(),
                    properties.clone(),
                    sequence,
                );
            }
            PatchOperation::DeclareRelation { fact } => {
                if !graph.relation_endpoint_exists(&fact.relation) {
                    return Err(ProjectionError::UnknownSubject(fact.relation.to_string()));
                }
                if !graph.declare_relation(fact.clone(), sequence) {
                    return Err(ProjectionError::DuplicateRelation(
                        fact.relation.to_string(),
                    ));
                }
            }
            PatchOperation::DeprecateSubject { subject, .. } => {
                if !graph.deprecate_subject(subject, sequence) {
                    return Err(ProjectionError::UnknownSubject(subject.to_string()));
                }
            }
        }
    }
    Ok(())
}
