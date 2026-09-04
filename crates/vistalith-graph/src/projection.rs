use vistalith_domain::{
    AuthorityClass, EventPayload, IntentOutcome, Namespace, PatchOperation, Provenance,
    RelationFact, RelationKind, RelationRef, SubjectKind, SubjectRef, VEvent,
};

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
        EventPayload::ThreadStarted(started) => {
            if graph.node(&started.thread).is_some() {
                return Err(ProjectionError::DuplicateSubject(
                    started.thread.to_string(),
                ));
            }
            graph.upsert_subject(
                started.thread.clone(),
                AuthorityClass::Authoritative,
                event_provenance(event),
                thread_properties(&[
                    ("title", serde_json::json!(started.title)),
                    ("turns", serde_json::json!(0)),
                ]),
                sequence,
            );
        }
        EventPayload::MessageAppended(appended) => {
            if graph.node(&appended.thread).is_none() {
                return Err(ProjectionError::UnknownSubject(appended.thread.to_string()));
            }
            if graph.node(&appended.message).is_some() {
                return Err(ProjectionError::DuplicateSubject(
                    appended.message.to_string(),
                ));
            }
            graph.upsert_subject(
                appended.message.clone(),
                AuthorityClass::Authoritative,
                event_provenance(event),
                thread_properties(&[
                    ("role", serde_json::json!(appended.role)),
                    ("content", serde_json::json!(appended.content)),
                    ("turn", serde_json::json!(appended.turn)),
                ]),
                sequence,
            );
            let fact = RelationFact {
                relation: RelationRef::new(
                    appended.thread.clone(),
                    RelationKind::Contains,
                    appended.message.clone(),
                )
                .map_err(|e| ProjectionError::InvalidOperation(e.to_string()))?,
                authority: AuthorityClass::Authoritative,
                provenance: event_provenance(event),
            };
            if !graph.declare_relation(fact, sequence) {
                return Err(ProjectionError::DuplicateRelation(format!(
                    "thread {} already contains {}",
                    appended.thread, appended.message
                )));
            }
        }
        EventPayload::TurnCompleted(turn) => {
            if graph.node(&turn.thread).is_none() {
                return Err(ProjectionError::UnknownSubject(turn.thread.to_string()));
            }
            // Thread progress is merged: replaying is idempotent per event.
            graph.update_subject(
                &turn.thread,
                &thread_properties(&[
                    ("turns", serde_json::json!(turn.turn)),
                    ("last_model", serde_json::json!(turn.model.to_string())),
                    (
                        "last_usage",
                        serde_json::json!({
                            "input_tokens": turn.usage.input_tokens,
                            "output_tokens": turn.usage.output_tokens,
                            "total_tokens": turn.usage.total_tokens,
                        }),
                    ),
                ]),
                sequence,
            );
            // Model usage becomes a derived observation: the model subject and
            // the `used_model` edge are Vistalith facts about a live call.
            let model_subject = SubjectRef::new(
                Namespace::Agentic,
                SubjectKind::Model,
                turn.model.subject_id(),
            )
            .map_err(|e| ProjectionError::InvalidOperation(e.to_string()))?;
            graph.upsert_subject(
                model_subject.clone(),
                AuthorityClass::Derived,
                event_provenance(event),
                thread_properties(&[
                    ("provider", serde_json::json!(turn.model.provider)),
                    ("model", serde_json::json!(turn.model.model)),
                ]),
                sequence,
            );
            if let Ok(relation) =
                RelationRef::new(turn.thread.clone(), RelationKind::UsedModel, model_subject)
            {
                let fact = RelationFact {
                    relation,
                    authority: AuthorityClass::Derived,
                    provenance: event_provenance(event),
                };
                // Idempotent: many turns may share one model.
                graph.declare_relation(fact, sequence);
            }
        }
        EventPayload::ToolInvoked(invoked) => {
            if graph.node(&invoked.thread).is_none() {
                return Err(ProjectionError::UnknownSubject(invoked.thread.to_string()));
            }
            if graph.node(&invoked.tool_call).is_some() {
                return Err(ProjectionError::DuplicateSubject(
                    invoked.tool_call.to_string(),
                ));
            }
            // Typed tool items stay structured: args and output are durable.
            graph.upsert_subject(
                invoked.tool_call.clone(),
                AuthorityClass::Derived,
                event_provenance(event),
                thread_properties(&[
                    ("tool", serde_json::json!(invoked.tool)),
                    ("args", invoked.args.clone()),
                    ("output", invoked.output.clone()),
                ]),
                sequence,
            );
            let fact = RelationFact {
                relation: RelationRef::new(
                    invoked.thread.clone(),
                    RelationKind::UsedTool,
                    invoked.tool_call.clone(),
                )
                .map_err(|e| ProjectionError::InvalidOperation(e.to_string()))?,
                authority: AuthorityClass::Derived,
                provenance: event_provenance(event),
            };
            graph.declare_relation(fact, sequence);
        }
        EventPayload::IntentDrafted(drafted) => {
            if graph.node(&drafted.target).is_none() {
                return Err(ProjectionError::UnknownSubject(drafted.target.to_string()));
            }
            if graph.node(&drafted.intent).is_some() {
                return Err(ProjectionError::DuplicateSubject(
                    drafted.intent.to_string(),
                ));
            }
            // SPEC-006: a gesture creates a draft only — advisory by class.
            let mut properties = thread_properties(&[
                ("gesture", serde_json::json!(drafted.gesture)),
                ("change", drafted.change.clone()),
                ("base_revision", serde_json::json!(drafted.base_revision)),
                ("status", serde_json::json!("draft")),
            ]);
            if let Some(reason) = &drafted.reason {
                properties.insert("reason".to_owned(), serde_json::json!(reason));
            }
            graph.upsert_subject(
                drafted.intent.clone(),
                AuthorityClass::Advisory,
                event_provenance(event),
                properties,
                sequence,
            );
            let fact = RelationFact {
                relation: RelationRef::new(
                    drafted.intent.clone(),
                    RelationKind::ProposesChangeTo,
                    drafted.target.clone(),
                )
                .map_err(|e| ProjectionError::InvalidOperation(e.to_string()))?,
                authority: AuthorityClass::Advisory,
                provenance: event_provenance(event),
            };
            if !graph.declare_relation(fact, sequence) {
                return Err(ProjectionError::DuplicateRelation(format!(
                    "{} proposes change to {}",
                    drafted.intent, drafted.target
                )));
            }
        }
        EventPayload::IntentPromoted(promoted) => {
            if graph.node(&promoted.intent).is_none() {
                return Err(ProjectionError::UnknownSubject(promoted.intent.to_string()));
            }
            let status = match &promoted.outcome {
                IntentOutcome::AppliedToGraph { .. } => "applied",
                IntentOutcome::RoutedToSddkGovernance { .. } => "sddk-governed",
                IntentOutcome::StaleBase { .. } => "stale",
                IntentOutcome::RejectedLocally { .. } => "rejected",
                IntentOutcome::Discarded { .. } => "discarded",
            };
            let mut properties = thread_properties(&[("status", serde_json::json!(status))]);
            match &promoted.outcome {
                IntentOutcome::AppliedToGraph { revision } => {
                    properties.insert("applied_revision".to_owned(), serde_json::json!(revision));
                }
                IntentOutcome::RejectedLocally { reason } => {
                    properties.insert("reject_reason".to_owned(), serde_json::json!(reason));
                }
                _ => {}
            }
            graph.update_subject(&promoted.intent, &properties, sequence);
        }
    }
    Ok(graph.bump_revision())
}

fn event_provenance(event: &VEvent) -> Provenance {
    Provenance {
        source: event.actor.clone(),
        source_revision: None,
        note: None,
        confidence: None,
    }
}

fn thread_properties(
    entries: &[(&str, serde_json::Value)],
) -> std::collections::BTreeMap<String, serde_json::Value> {
    entries
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
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
