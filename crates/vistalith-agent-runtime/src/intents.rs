//! Visual intent lifecycle (SPEC-006, `visual/VISUAL-INTENT.md`).
//!
//! ```text
//! gesture → IntentDrafted (draft only) → preview (stale-aware)
//!         → explicit promotion → graph patch | SDDK governance | discarded
//! ```
//!
//! A draft never executes anything: it resolves a semantic target, records
//! the proposed operations and the base graph revision. Promotion is an
//! explicit, separate act.

use thiserror::Error;
use vistalith_domain::{
    Actor, EventPayload, IntentDrafted, IntentOutcome, IntentPromoted, Namespace, PatchId,
    PatchOperation, SubjectKind, SubjectRef,
};
use vistalith_graph::{GraphPatch, GraphStore, PatchOutcome};

#[derive(Debug, Error)]
pub enum IntentError {
    #[error("target subject `{0}` does not exist")]
    UnknownTarget(String),
    #[error("intent `{0}` does not exist")]
    UnknownIntent(String),
    #[error("intent change payload does not carry valid patch operations: {0}")]
    InvalidChange(String),
    #[error("intent `{0}` was already resolved ({1})")]
    AlreadyResolved(String, &'static str),
    #[error(transparent)]
    Store(#[from] vistalith_graph::StoreError),
}

/// The result of promoting a draft.
#[derive(Debug, Clone, PartialEq)]
pub enum Promotion {
    Applied {
        revision: u64,
    },
    RoutedToSddkGovernance {
        subject: SubjectRef,
    },
    SubmittedToSddk {
        subject: SubjectRef,
        proposal: SubjectRef,
        receipt_id: Option<String>,
        decision: String,
    },
    Stale {
        current_revision: u64,
        base_revision: u64,
    },
    RejectedLocally {
        reason: String,
    },
}

/// Drafts an intent against `target` at the current graph revision.
pub fn draft_intent(
    store: &mut GraphStore,
    target: &SubjectRef,
    gesture: impl Into<String>,
    change: serde_json::Value,
    reason: Option<String>,
    actor: &Actor,
) -> Result<SubjectRef, IntentError> {
    if store.graph().subject(target).is_none() {
        return Err(IntentError::UnknownTarget(target.to_string()));
    }
    let intent = SubjectRef::new(
        Namespace::Visual,
        SubjectKind::VisualProposal,
        uuid::Uuid::now_v7().to_string(),
    )
    .expect("generated intent id is valid");
    // The draft event itself bumps the graph revision, so the base the draft
    // is previewed against is the revision its own event produces.
    let base_revision = store.graph().revision() + 1;
    store.append(event(
        actor,
        EventPayload::IntentDrafted(IntentDrafted {
            intent: intent.clone(),
            target: target.clone(),
            gesture: gesture.into(),
            change,
            base_revision,
            reason,
        }),
        vec![intent.clone(), target.clone()],
    ))?;
    debug_assert_eq!(store.graph().revision(), base_revision);
    Ok(intent)
}

/// Promotes a draft (explicit user/agent act). Stale-aware: if the graph
/// moved on since the draft, promotion is denied and recorded as stale.
pub fn promote_intent(
    store: &mut GraphStore,
    intent: &SubjectRef,
    actor: &Actor,
) -> Result<Promotion, IntentError> {
    promote_intent_with_bridge(store, intent, actor, None, false)
}

/// Promotes a draft with the SDDK governance bridge available (SPK-012):
/// SDDK-owned targets route through SDDK's capability gateway — the
/// decision and receipt are durable — instead of the bare governance
/// routing. `approve` supplies explicit human approval for capabilities the
/// SDDK workflow marks as high risk.
pub fn promote_intent_with_bridge(
    store: &mut GraphStore,
    intent: &SubjectRef,
    actor: &Actor,
    bridge: Option<&vistalith_sddk_bridge::SddkBridge>,
    approve: bool,
) -> Result<Promotion, IntentError> {
    let node = store
        .graph()
        .subject(intent)
        .ok_or_else(|| IntentError::UnknownIntent(intent.to_string()))?;
    let status = node.properties.get("status").and_then(|v| v.as_str());
    if status.is_some_and(|status| status != "draft") {
        return Err(IntentError::AlreadyResolved(
            intent.to_string(),
            match status {
                Some("applied") => "applied",
                Some("sddk-governed") => "sddk-governed",
                Some("stale") => "stale",
                Some("rejected") => "rejected",
                _ => "discarded",
            },
        ));
    }
    let base_revision = node
        .properties
        .get("base_revision")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| IntentError::InvalidChange("intent has no base_revision".to_owned()))?;
    let change = node
        .properties
        .get("change")
        .cloned()
        .ok_or_else(|| IntentError::InvalidChange("intent has no change payload".to_owned()))?;
    let note = node
        .properties
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("visual intent promotion")
        .to_owned();

    // Stale-aware promotion (SPEC-006): the preview must match the graph.
    let current_revision = store.graph().revision();
    if current_revision != base_revision {
        record(
            store,
            actor,
            intent,
            IntentOutcome::StaleBase { current_revision },
        )?;
        return Ok(Promotion::Stale {
            current_revision,
            base_revision,
        });
    }

    let operations: Vec<PatchOperation> = serde_json::from_value(
        change
            .get("operations")
            .cloned()
            .ok_or_else(|| IntentError::InvalidChange("missing `operations`".into()))?,
    )
    .map_err(|e| IntentError::InvalidChange(e.to_string()))?;

    let target = store
        .graph()
        .outgoing(intent)
        .find(|f| f.relation.kind.as_str() == "proposes_change_to")
        .map(|f| f.relation.to.clone());
    let target = target.expect("drafted intents always carry proposes_change_to");

    let patch = GraphPatch {
        patch_id: PatchId::generate(),
        base_revision,
        proposed_by: actor.clone(),
        operations,
    };
    match store.propose_patch(patch)? {
        PatchOutcome::Applied { revision, .. } => {
            record(
                store,
                actor,
                intent,
                IntentOutcome::AppliedToGraph { revision },
            )?;
            Ok(Promotion::Applied { revision })
        }
        PatchOutcome::Rejected { reason, .. } => {
            // SDDK-owned targets route to governance: the semantic change
            // proposal becomes SDDK-governed work through SDDK's own flow.
            if reason.contains("governed SDDK") {
                if let Some(bridge) = bridge {
                    // SPK-012 / M7: propose through the SDDK capability
                    // gateway. The decision and the SDDK receipt are durable
                    // in the sddk-proposal-submitted event.
                    let bundle = serde_json::json!({
                        "artifacts": [],
                        "environment": {},
                        "execution": {},
                        "change": change,
                    });
                    let proposal = bridge
                        .submit_evidence_proposal(
                            store,
                            intent,
                            &target,
                            bundle,
                            note,
                            actor,
                            approve,
                        )
                        .map_err(|e| {
                            IntentError::InvalidChange(format!("sddk bridge: {e}"))
                        })?;
                    let proposal_node = store
                        .graph()
                        .subject(&proposal)
                        .ok_or_else(|| {
                            IntentError::InvalidChange(
                                "bridge proposal missing from projection".to_owned(),
                            )
                        })?;
                    let decision = proposal_node
                        .properties
                        .get("decision")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_owned();
                    let receipt_id = proposal_node
                        .properties
                        .get("receipt_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned);
                    record(
                        store,
                        actor,
                        intent,
                        IntentOutcome::SubmittedToSddk {
                            subject: target.clone(),
                            proposal: proposal.clone(),
                            receipt_id: receipt_id.clone(),
                            decision: decision.clone(),
                        },
                    )?;
                    return Ok(Promotion::SubmittedToSddk {
                        subject: target,
                        proposal,
                        receipt_id,
                        decision,
                    });
                }
                record(
                    store,
                    actor,
                    intent,
                    IntentOutcome::RoutedToSddkGovernance {
                        subject: target.clone(),
                    },
                )?;
                Ok(Promotion::RoutedToSddkGovernance { subject: target })
            } else {
                record(
                    store,
                    actor,
                    intent,
                    IntentOutcome::RejectedLocally {
                        reason: reason.clone(),
                    },
                )?;
                Ok(Promotion::RejectedLocally { reason })
            }
        }
    }
}

/// Discards a draft; nothing executes.
pub fn discard_intent(
    store: &mut GraphStore,
    intent: &SubjectRef,
    reason: Option<String>,
    actor: &Actor,
) -> Result<(), IntentError> {
    if store.graph().subject(intent).is_none() {
        return Err(IntentError::UnknownIntent(intent.to_string()));
    }
    record(store, actor, intent, IntentOutcome::Discarded { reason })
}

fn record(
    store: &mut GraphStore,
    actor: &Actor,
    intent: &SubjectRef,
    outcome: IntentOutcome,
) -> Result<(), IntentError> {
    store.append(event(
        actor,
        EventPayload::IntentPromoted(IntentPromoted {
            intent: intent.clone(),
            outcome,
        }),
        vec![intent.clone()],
    ))?;
    Ok(())
}

fn event(
    actor: &Actor,
    payload: EventPayload,
    subjects: Vec<SubjectRef>,
) -> vistalith_domain::VEvent {
    vistalith_domain::VEvent {
        event_id: uuid::Uuid::now_v7(),
        actor: actor.clone(),
        timestamp: time::OffsetDateTime::now_utc(),
        subjects,
        correlation_id: uuid::Uuid::now_v7(),
        causation_id: None,
        trace_id: None,
        payload,
    }
}
