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
            // SPEC-011: copied items keep a binding back to their original.
            if let Some(original) = &appended.forked_of {
                graph.update_subject(
                    &appended.message,
                    &thread_properties(&[("forked_of", serde_json::json!(original.to_string()))]),
                    sequence,
                );
            }
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
            let mut tool_properties = thread_properties(&[
                ("tool", serde_json::json!(invoked.tool)),
                ("args", invoked.args.clone()),
                ("output", invoked.output.clone()),
            ]);
            if let Some(source) = &invoked.source {
                tool_properties.insert("source".to_owned(), serde_json::json!(source));
            }
            if let Some(original) = &invoked.forked_of {
                tool_properties
                    .insert("forked_of".to_owned(), serde_json::json!(original.to_string()));
            }
            graph.upsert_subject(
                invoked.tool_call.clone(),
                AuthorityClass::Derived,
                event_provenance(event),
                tool_properties,
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
        EventPayload::ThreadForked(forked) => {
            let source = graph
                .subject(&forked.source)
                .ok_or_else(|| ProjectionError::UnknownSubject(forked.source.to_string()))?;
            if graph.node(&forked.fork).is_some() {
                return Err(ProjectionError::DuplicateSubject(forked.fork.to_string()));
            }
            // The fork is a first-class durable thread; its title derives
            // from the source so replay alone reconstructs the lens state.
            let source_title = source
                .properties
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("thread");
            graph.upsert_subject(
                forked.fork.clone(),
                AuthorityClass::Authoritative,
                event_provenance(event),
                thread_properties(&[
                    (
                        "title",
                        serde_json::json!(format!(
                            "{source_title} (fork ≤ turn {})",
                            forked.up_to_turn
                        )),
                    ),
                    ("turns", serde_json::json!(forked.up_to_turn)),
                    ("forked_from", serde_json::json!(forked.source.to_string())),
                ]),
                sequence,
            );
            if let Some(note) = &forked.note {
                graph.update_subject(
                    &forked.fork,
                    &thread_properties(&[("note", serde_json::json!(note))]),
                    sequence,
                );
            }
            let fact = RelationFact {
                relation: RelationRef::new(
                    forked.fork.clone(),
                    RelationKind::ForkedFrom,
                    forked.source.clone(),
                )
                .map_err(|e| ProjectionError::InvalidOperation(e.to_string()))?,
                authority: AuthorityClass::Authoritative,
                provenance: event_provenance(event),
            };
            if !graph.declare_relation(fact, sequence) {
                return Err(ProjectionError::DuplicateRelation(format!(
                    "{} forked from {}",
                    forked.fork, forked.source
                )));
            }
        }
        EventPayload::AdvisoryRaised(raised) => {
            if graph.node(&raised.about).is_none() {
                return Err(ProjectionError::UnknownSubject(raised.about.to_string()));
            }
            if graph.node(&raised.advisory).is_some() {
                return Err(ProjectionError::DuplicateSubject(raised.advisory.to_string()));
            }
            // SPEC-003: behavior outputs are advisory-class facts about the
            // graph, never authoritative mutations of it.
            graph.upsert_subject(
                raised.advisory.clone(),
                AuthorityClass::Advisory,
                event_provenance(event),
                thread_properties(&[
                    ("behavior", serde_json::json!(raised.behavior)),
                    ("note", serde_json::json!(raised.note)),
                    ("about", serde_json::json!(raised.about.to_string())),
                ]),
                sequence,
            );
            let fact = RelationFact {
                relation: RelationRef::new(
                    raised.advisory.clone(),
                    RelationKind::Mentions,
                    raised.about.clone(),
                )
                .map_err(|e| ProjectionError::InvalidOperation(e.to_string()))?,
                authority: AuthorityClass::Advisory,
                provenance: event_provenance(event),
            };
            if !graph.declare_relation(fact, sequence) {
                return Err(ProjectionError::DuplicateRelation(format!(
                    "{} mentions {}",
                    raised.advisory, raised.about
                )));
            }
        }
        EventPayload::SddkProposalSubmitted(submitted) => {
            if graph.node(&submitted.intent).is_none() {
                return Err(ProjectionError::UnknownSubject(submitted.intent.to_string()));
            }
            if graph.node(&submitted.target).is_none() {
                return Err(ProjectionError::UnknownSubject(submitted.target.to_string()));
            }
            if graph.node(&submitted.proposal).is_some() {
                return Err(ProjectionError::DuplicateSubject(
                    submitted.proposal.to_string(),
                ));
            }
            // SPK-012: the proposal is a Vistalith-owned DERIVED observation
            // of an SDDK-side fact — never authoritative SDDK state.
            graph.upsert_subject(
                submitted.proposal.clone(),
                AuthorityClass::Derived,
                event_provenance(event),
                thread_properties(&[
                    ("capability", serde_json::json!(submitted.capability)),
                    ("decision", serde_json::json!(submitted.decision)),
                    (
                        "receipt_id",
                        submitted
                            .receipt_id
                            .clone()
                            .map(serde_json::Value::String)
                            .unwrap_or(serde_json::Value::Null),
                    ),
                    ("receipt", submitted.receipt.clone()),
                    ("intent", serde_json::json!(submitted.intent.to_string())),
                ]),
                sequence,
            );
            // The proposal is evidence for the SDDK subject it targets.
            let fact = RelationFact {
                relation: RelationRef::new(
                    submitted.proposal.clone(),
                    RelationKind::ProvidesEvidenceFor,
                    submitted.target.clone(),
                )
                .map_err(|e| ProjectionError::InvalidOperation(e.to_string()))?,
                authority: AuthorityClass::Derived,
                provenance: event_provenance(event),
            };
            if !graph.declare_relation(fact, sequence) {
                return Err(ProjectionError::DuplicateRelation(format!(
                    "{} provides evidence for {}",
                    submitted.proposal, submitted.target
                )));
            }
        }
        EventPayload::UatCheckRecorded(recorded) => {
            if graph.node(&recorded.scenario).is_none() {
                return Err(ProjectionError::UnknownSubject(
                    recorded.scenario.to_string(),
                ));
            }
            if graph.node(&recorded.check).is_some() {
                return Err(ProjectionError::DuplicateSubject(
                    recorded.check.to_string(),
                ));
            }
            let verdict = serde_json::to_value(recorded.verdict)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let mut properties = thread_properties(&[
                ("verdict", serde_json::json!(verdict)),
                ("scenario", serde_json::json!(recorded.scenario.to_string())),
            ]);
            if let Some(evidence_ref) = &recorded.evidence_ref {
                properties
                    .insert("evidence_ref".to_owned(), serde_json::json!(evidence_ref));
            }
            if let Some(note) = &recorded.note {
                properties.insert("note".to_owned(), serde_json::json!(note));
            }
            graph.upsert_subject(
                recorded.check.clone(),
                AuthorityClass::Authoritative,
                event_provenance(event),
                properties,
                sequence,
            );
            // Traceability: check -[tested_by... actually scenario
            // -[verified_by]-> check reads backwards; the scenario
            // -[contains]-> check relation keeps the inventory per scenario.
            let fact = RelationFact {
                relation: RelationRef::new(
                    recorded.scenario.clone(),
                    RelationKind::Contains,
                    recorded.check.clone(),
                )
                .map_err(|e| ProjectionError::InvalidOperation(e.to_string()))?,
                authority: AuthorityClass::Authoritative,
                provenance: event_provenance(event),
            };
            if !graph.declare_relation(fact, sequence) {
                return Err(ProjectionError::DuplicateRelation(format!(
                    "{} contains {}",
                    recorded.scenario, recorded.check
                )));
            }
        }
        EventPayload::AgentRunFinished(run) => {
            for subject in [&run.agent, &run.frame] {
                if graph.node(subject).is_none() {
                    return Err(ProjectionError::UnknownSubject(subject.to_string()));
                }
            }
            if graph.node(&run.run).is_some() {
                return Err(ProjectionError::DuplicateSubject(run.run.to_string()));
            }
            let mut properties = thread_properties(&[
                ("agent", serde_json::json!(run.agent.to_string())),
                ("frame", serde_json::json!(run.frame.to_string())),
                ("status", serde_json::json!(run.status)),
            ]);
            if !run.findings.is_empty() {
                properties
                    .insert("findings".to_owned(), serde_json::json!(run.findings));
            }
            if !run.risks.is_empty() {
                properties.insert("risks".to_owned(), serde_json::json!(run.risks));
            }
            if !run.assumptions.is_empty() {
                properties
                    .insert("assumptions".to_owned(), serde_json::json!(run.assumptions));
            }
            graph.upsert_subject(
                run.run.clone(),
                AuthorityClass::Authoritative,
                event_provenance(event),
                properties,
                sequence,
            );
            // Traceability edges: run contributes to the frame, is
            // executed_by the agent.
            for (kind, to) in [
                (RelationKind::ContributesTo, run.frame.clone()),
                (RelationKind::ExecutedBy, run.agent.clone()),
            ] {
                let fact = RelationFact {
                    relation: RelationRef::new(run.run.clone(), kind, to)
                        .map_err(|e| ProjectionError::InvalidOperation(e.to_string()))?,
                    authority: AuthorityClass::Authoritative,
                    provenance: event_provenance(event),
                };
                graph.declare_relation(fact, sequence);
            }
        }
        EventPayload::AgentDefined(defined) => {
            if graph.node(&defined.agent).is_some() {
                return Err(ProjectionError::DuplicateSubject(
                    defined.agent.to_string(),
                ));
            }
            let mut properties = thread_properties(&[
                ("role", serde_json::json!(defined.role)),
                ("instructions", serde_json::json!(defined.instructions)),
                ("tools", serde_json::json!(defined.tools)),
                (
                    "expected_outputs",
                    serde_json::json!(defined.expected_outputs),
                ),
            ]);
            if let Some(model) = &defined.model {
                properties.insert("model".to_owned(), serde_json::json!(model.to_string()));
            }
            if let Some(budget) = defined.budget_turns {
                properties.insert("budget_turns".to_owned(), serde_json::json!(budget));
            }
            graph.upsert_subject(
                defined.agent.clone(),
                AuthorityClass::Authoritative,
                event_provenance(event),
                properties,
                sequence,
            );
        }
        EventPayload::FrameStarted(started) => {
            if graph.node(&started.frame).is_some() {
                return Err(ProjectionError::DuplicateSubject(
                    started.frame.to_string(),
                ));
            }
            if let Some(agent) = &started.agent
                && graph.node(agent).is_none()
            {
                return Err(ProjectionError::UnknownSubject(agent.to_string()));
            }
            for subject in &started.subjects {
                if graph.node(subject).is_none() {
                    return Err(ProjectionError::UnknownSubject(subject.to_string()));
                }
            }
            graph.upsert_subject(
                started.frame.clone(),
                AuthorityClass::Authoritative,
                event_provenance(event),
                thread_properties(&[
                    ("goal", serde_json::json!(started.goal)),
                    ("status", serde_json::json!("open")),
                    ("max_turns", serde_json::json!(started.max_turns)),
                    ("token_budget", serde_json::json!(started.token_budget)),
                    ("turns", serde_json::json!(0)),
                    ("used_tokens", serde_json::json!(0)),
                    (
                        "permitted_tools",
                        serde_json::json!(started.permitted_tools),
                    ),
                ]),
                sequence,
            );
            if let Some(agent) = &started.agent {
                let fact = RelationFact {
                    relation: RelationRef::new(
                        started.frame.clone(),
                        RelationKind::DelegatedTo,
                        agent.clone(),
                    )
                    .map_err(|e| ProjectionError::InvalidOperation(e.to_string()))?,
                    authority: AuthorityClass::Authoritative,
                    provenance: event_provenance(event),
                };
                if !graph.declare_relation(fact, sequence) {
                    return Err(ProjectionError::DuplicateRelation(format!(
                        "{} delegated to {}",
                        started.frame, agent
                    )));
                }
            }
            for subject in &started.subjects {
                let fact = RelationFact {
                    relation: RelationRef::new(
                        started.frame.clone(),
                        RelationKind::Mentions,
                        subject.clone(),
                    )
                    .map_err(|e| ProjectionError::InvalidOperation(e.to_string()))?,
                    authority: AuthorityClass::Authoritative,
                    provenance: event_provenance(event),
                };
                if !graph.declare_relation(fact, sequence) {
                    return Err(ProjectionError::DuplicateRelation(format!(
                        "{} mentions {}",
                        started.frame, subject
                    )));
                }
            }
        }
        EventPayload::FrameTurnCompleted(turn) => {
            let node = graph
                .subject(&turn.frame)
                .ok_or_else(|| ProjectionError::UnknownSubject(turn.frame.to_string()))?;
            let previous = node
                .properties
                .get("used_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            graph.update_subject(
                &turn.frame,
                &thread_properties(&[
                    ("turns", serde_json::json!(turn.turn)),
                    (
                        "used_tokens",
                        serde_json::json!(previous + turn.usage.total_tokens),
                    ),
                    ("last_model", serde_json::json!(turn.model.to_string())),
                ]),
                sequence,
            );
            // Model usage becomes a derived observation, like thread turns.
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
                RelationRef::new(turn.frame.clone(), RelationKind::UsedModel, model_subject)
            {
                graph.declare_relation(
                    RelationFact {
                        relation,
                        authority: AuthorityClass::Derived,
                        provenance: event_provenance(event),
                    },
                    sequence,
                );
            }
        }
        EventPayload::FrameClosed(closed) => {
            if graph.node(&closed.frame).is_none() {
                return Err(ProjectionError::UnknownSubject(closed.frame.to_string()));
            }
            let outcome = match closed.outcome {
                vistalith_domain::FrameOutcome::Completed => "completed",
                vistalith_domain::FrameOutcome::Aborted => "aborted",
                vistalith_domain::FrameOutcome::TurnsExhausted => "turns-exhausted",
                vistalith_domain::FrameOutcome::BudgetExhausted => "budget-exhausted",
            };
            let mut properties = thread_properties(&[
                ("status", serde_json::json!(outcome)),
                ("outcome", serde_json::json!(outcome)),
            ]);
            if let Some(summary) = &closed.summary {
                properties.insert("summary".to_owned(), serde_json::json!(summary));
            }
            graph.update_subject(&closed.frame, &properties, sequence);
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
                IntentOutcome::SubmittedToSddk { .. } => "submitted",
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
                IntentOutcome::SubmittedToSddk {
                    subject,
                    receipt_id,
                    decision,
                    ..
                } => {
                    properties.insert("sddk_subject".to_owned(), serde_json::json!(subject.to_string()));
                    properties
                        .insert("sddk_receipt_id".to_owned(), receipt_id.clone().into());
                    properties.insert("sddk_decision".to_owned(), serde_json::json!(decision));
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
