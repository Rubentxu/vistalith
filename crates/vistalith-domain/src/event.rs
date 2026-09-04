use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::authority::AuthorityClass;
use crate::provenance::Provenance;
use crate::subject::SubjectRef;

/// Determinism class of a behavior (`graph/EVENT-SOURCED-GRAPH.md`).
///
/// The class is explicit per behavior: only `DeterministicProjection` and
/// `DeterministicRule` behaviors participate in strict fixture replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeterminismClass {
    DeterministicProjection,
    DeterministicRule,
    RecordedExternal,
    LiveNonDeterministic,
}

/// Typed payload of a durable Vistalith event (SPEC-002).
///
/// Serialized with an adjacent `type` tag so the durable JSON matches the
/// `VEvent` shape in `graph/EVENT-SOURCED-GRAPH.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "kebab-case")]
pub enum EventPayload {
    /// Defines a semantic subject with its authority class and provenance.
    SubjectDefined(SubjectDefined),
    /// Merges properties into an existing subject.
    SubjectUpdated(SubjectUpdated),
    /// Marks a subject deprecated; it remains in the graph, distinguishable.
    SubjectDeprecated(SubjectDeprecated),
    /// Declares a typed relation between two existing subjects.
    RelationDeclared(RelationDeclared),
    /// A graph patch was applied (its operations included).
    PatchApplied(PatchApplied),
    /// A graph patch was rejected; rejections are durable events too
    /// (SPEC-002: failures and rejected patches are events).
    PatchRejected(PatchRejected),
    /// A conversation thread was started (SPEC-007: threads are durable
    /// Vistalith state).
    ThreadStarted(ThreadStarted),
    /// A typed conversation item was appended to a thread.
    MessageAppended(MessageAppended),
    /// A conversation turn completed: model identity and token usage are
    /// durable (SPEC-008 traceability).
    TurnCompleted(TurnCompleted),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubjectDefined {
    pub subject: SubjectRef,
    pub authority: AuthorityClass,
    pub provenance: Provenance,
    #[serde(default, skip_serializing_if = "map_is_empty")]
    pub properties: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubjectUpdated {
    pub subject: SubjectRef,
    pub properties: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubjectDeprecated {
    pub subject: SubjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationDeclared {
    pub fact: crate::relation::RelationFact,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchApplied {
    pub patch_id: crate::patch::PatchId,
    pub operations: Vec<crate::patch::PatchOperation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchRejected {
    pub patch_id: crate::patch::PatchId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadStarted {
    pub thread: SubjectRef,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageAppended {
    pub thread: SubjectRef,
    pub message: SubjectRef,
    pub role: crate::model::MessageRole,
    pub content: String,
    /// 1-based turn counter inside the thread.
    pub turn: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnCompleted {
    pub thread: SubjectRef,
    pub turn: u64,
    pub model: crate::model::ModelDescriptor,
    pub usage: crate::model::ModelUsage,
}

fn map_is_empty(map: &BTreeMap<String, serde_json::Value>) -> bool {
    map.is_empty()
}

/// A durable Vistalith event (SPEC-002 / `graph/EVENT-SOURCED-GRAPH.md`).
///
/// `sequence` and `revision` are assigned by the durable log at append time
/// and are carried by [`StoredEvent`]; the appendable event itself does not
/// contain them, which is what makes raw fixture logs replayable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VEvent {
    pub event_id: Uuid,
    pub actor: crate::provenance::Actor,
    /// RFC3339 UTC timestamp; `serde(with)` keeps fixture JSON human-readable.
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
    /// Every subject this event touches (aggregate/subject refs).
    pub subjects: Vec<SubjectRef>,
    pub correlation_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(flatten)]
    pub payload: EventPayload,
}

impl VEvent {
    pub fn kind(&self) -> &'static str {
        match self.payload {
            EventPayload::SubjectDefined(_) => "subject-defined",
            EventPayload::SubjectUpdated(_) => "subject-updated",
            EventPayload::SubjectDeprecated(_) => "subject-deprecated",
            EventPayload::RelationDeclared(_) => "relation-declared",
            EventPayload::PatchApplied(_) => "patch-applied",
            EventPayload::PatchRejected(_) => "patch-rejected",
            EventPayload::ThreadStarted(_) => "thread-started",
            EventPayload::MessageAppended(_) => "message-appended",
            EventPayload::TurnCompleted(_) => "turn-completed",
        }
    }
}

impl fmt::Display for VEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.kind(), self.event_id)
    }
}

/// A durable log entry: the event plus the log-assigned position and the
/// graph revision the event produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredEvent {
    pub sequence: u64,
    pub revision: u64,
    #[serde(flatten)]
    pub event: VEvent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation::RelationKind;
    use crate::subject::{Namespace, SubjectKind, SubjectRef};
    use crate::{Actor, RelationRef};

    fn container(id: &str) -> SubjectRef {
        SubjectRef::new(Namespace::Arch, SubjectKind::Container, id).unwrap()
    }

    fn sample_event() -> VEvent {
        let fact = crate::relation::RelationFact {
            relation: RelationRef::new(
                container("payment-service"),
                RelationKind::DependsOn,
                container("ledger"),
            )
            .unwrap(),
            authority: AuthorityClass::Authoritative,
            provenance: Provenance::new("user:ruben").unwrap(),
        };
        VEvent {
            event_id: Uuid::now_v7(),
            actor: Actor::new("user:ruben").unwrap(),
            timestamp: OffsetDateTime::parse(
                "2026-09-04T09:00:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
            subjects: vec![container("payment-service"), container("ledger")],
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            trace_id: Some("fixture".to_owned()),
            payload: EventPayload::RelationDeclared(RelationDeclared { fact }),
        }
    }

    #[test]
    fn serde_roundtrip_matches_baseline_shape() {
        let event = sample_event();
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "relation-declared");
        assert!(json["payload"]["fact"]["relation"].is_object());
        let back: VEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn stored_event_wraps_raw_event() {
        let event = sample_event();
        let stored = StoredEvent {
            sequence: 3,
            revision: 4,
            event: event.clone(),
        };
        let json = serde_json::to_value(&stored).unwrap();
        assert_eq!(json["sequence"], 3);
        assert_eq!(json["revision"], 4);
        assert_eq!(json["type"], "relation-declared");
        let back: StoredEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back.event, event);
    }
}
