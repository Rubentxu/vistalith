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
    /// A native tool was invoked during a turn; the typed call and its
    /// output are durable (tools are never flattened into prose).
    ToolInvoked(ToolInvoked),
    /// A thread was forked at a turn boundary (SPEC-011): the fork is a new
    /// durable thread whose copied items keep semantic subject bindings.
    ThreadForked(ThreadForked),
    /// A reactive behavior raised an advisory (SPEC-003): behavior outputs
    /// are events, never hidden side effects, and never authoritative.
    AdvisoryRaised(AdvisoryRaised),
    /// A Vistalith agent was registered (`agentic/AGENTS-DELEGATION.md`):
    /// role, instructions, model profile, tools and expected outputs.
    AgentDefined(AgentDefined),
    /// A governed SDDK proposal went through the SDDK capability gateway
    /// (SPK-012): the decision and the SDDK receipt are durable here, so the
    /// promotion is traceable end to end (milestone M7).
    SddkProposalSubmitted(SddkProposalSubmitted),
    /// A UAT check was recorded against a scenario (UAT-STUDIO.md): verdict,
    /// optional evidence reference and notes — graph traceability to the
    /// scenario/work item without defining a parallel UAT lifecycle.
    UatCheckRecorded(UatCheckRecorded),
    /// An agent run finished (`agentic/AGENTS-DELEGATION.md`): structured
    /// outputs (findings/risks/assumptions) as durable typed items, never
    /// flattened to prose.
    AgentRunFinished(AgentRunFinished),
    /// A frame started (`graph/PATTERNS-VIEWS-FRAMES.md`): a bounded
    /// execution context — goal, subjects, permitted tools, budgets.
    FrameStarted(FrameStarted),
    /// A turn inside a frame completed; frame-level usage accounting.
    FrameTurnCompleted(FrameTurnCompleted),
    /// A frame reached a terminal state (explicit close or budget/turn
    /// exhaustion); closed frames accept no further turns.
    FrameClosed(FrameClosed),
    /// A visual gesture produced an intent draft (SPEC-006: drafts only —
    /// nothing executes until explicit promotion).
    IntentDrafted(IntentDrafted),
    /// An intent draft was promoted, routed to SDDK governance or discarded.
    IntentPromoted(IntentPromoted),
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
    /// Original message this item was copied from in a thread fork
    /// (SPEC-011: forks preserve semantic subject bindings). Absent on
    /// ordinary messages, so older logs stay readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_of: Option<SubjectRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnCompleted {
    pub thread: SubjectRef,
    pub turn: u64,
    pub model: crate::model::ModelDescriptor,
    pub usage: crate::model::ModelUsage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInvoked {
    pub thread: SubjectRef,
    pub tool_call: SubjectRef,
    /// Tool descriptor id, e.g. `graph_search`.
    pub tool: String,
    /// Tool source, e.g. `native` or `mcp:echo` (TOOLS-PERMISSIONS:
    /// descriptors carry their origin). Absent on older logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub args: serde_json::Value,
    pub output: serde_json::Value,
    /// Original tool-call subject this item was copied from in a thread
    /// fork (SPEC-011 binding preservation). Absent on live calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_of: Option<SubjectRef>,
}

/// Structured outputs of a finished agent run (AGENTS-DELEGATION.md
/// "Outputs"). Each item is a durable typed entry; the run subject links
/// the frame, the agent and the outputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRunFinished {
    pub run: SubjectRef,
    pub agent: SubjectRef,
    pub frame: SubjectRef,
    /// findings / risks / assumptions / alternatives, each with its text.
    pub findings: Vec<String>,
    pub risks: Vec<String>,
    pub assumptions: Vec<String>,
    /// Verdict marker for quick lens filtering.
    pub status: String,
}

/// A recorded UAT check (UAT-STUDIO.md). The check is a Vistalith-owned
/// human verification fact about a scenario; SDDK UAT semantics remain
/// authoritative wherever the scenario is SDDK-governed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UatCheckRecorded {
    /// The human-check subject this event creates.
    pub check: SubjectRef,
    /// The UAT scenario being verified.
    pub scenario: SubjectRef,
    /// pass | fail | blocked.
    pub verdict: UatVerdict,
    /// Evidence reference (artifact id, digest or URI) when captured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Verdict of a UAT check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UatVerdict {
    Pass,
    Fail,
    Blocked,
}

/// A governed SDDK proposal (SPK-012 / milestone M7). The proposal subject
/// is a Vistalith-owned observation of an SDDK-side fact: the decision and
/// the SDDK receipt (serialized) are durable provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SddkProposalSubmitted {
    pub proposal: SubjectRef,
    pub intent: SubjectRef,
    pub target: SubjectRef,
    /// SDDK capability exercised, e.g. `evidence.write`.
    pub capability: String,
    /// Gateway decision: `allowed`, `denied` or `approval-required`.
    pub decision: String,
    /// SDDK receipt id when the capability executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
    /// Full SDDK receipt (or the denial/approval payload) as JSON.
    pub receipt: serde_json::Value,
}

/// A Vistalith agent (`agentic/AGENTS-DELEGATION.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentDefined {
    pub agent: SubjectRef,
    pub role: String,
    pub instructions: String,
    /// Model profile this agent prefers, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<crate::model::ModelDescriptor>,
    /// Tool catalog ids the agent may be granted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Turn budget for runs of this agent, if bounded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_turns: Option<u32>,
    /// Names of expected structured outputs (declarative in v1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_outputs: Vec<String>,
}

/// A frame: bounded execution context owned by Vistalith
/// (`graph/PATTERNS-VIEWS-FRAMES.md`). All bounds are durable frame
/// properties; turns run inside a frame-owned thread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameStarted {
    pub frame: SubjectRef,
    pub goal: String,
    /// Agent the frame delegates execution to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<SubjectRef>,
    /// Semantic subjects the frame's context is built from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<SubjectRef>,
    /// Tool catalog ids permitted inside the frame; empty means no tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permitted_tools: Vec<String>,
    pub max_turns: u32,
    pub token_budget: u64,
}

/// Frame-level accounting for one turn inside the frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameTurnCompleted {
    pub frame: SubjectRef,
    pub turn: u64,
    pub model: crate::model::ModelDescriptor,
    pub usage: crate::model::ModelUsage,
}

/// Terminal outcome of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FrameOutcome {
    Completed,
    Aborted,
    TurnsExhausted,
    BudgetExhausted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameClosed {
    pub frame: SubjectRef,
    pub outcome: FrameOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// An advisory raised by a reactive behavior (SPEC-003). The advisory is a
/// Vistalith-owned, advisory-class subject linked to the subject it is about
/// with a `mentions` relation; raising it never mutates SDDK truth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdvisoryRaised {
    /// The advisory subject this event creates.
    pub advisory: SubjectRef,
    /// The subject the advisory is about.
    pub about: SubjectRef,
    /// Behavior identity, e.g. `impact-advisory@1`.
    pub behavior: String,
    pub note: String,
}

/// A thread fork (SPEC-011): a new thread carrying the source's durable
/// items up to `up_to_turn`, linked back with a `forked_from` relation.
/// Forks are advisory exploration state; promotion into SDDK stays explicit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadForked {
    /// The newly created fork thread.
    pub fork: SubjectRef,
    /// The thread the items were copied from.
    pub source: SubjectRef,
    /// Last source turn carried into the fork (1-based; the fork's `turns`
    /// property equals this).
    pub up_to_turn: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentDrafted {
    pub intent: SubjectRef,
    /// Semantic subject the gesture resolved to (SPEC-006: intents resolve
    /// subjects, not renderer shapes).
    pub target: SubjectRef,
    /// Gesture type, e.g. `rename`, `connect`, `annotate`.
    pub gesture: String,
    /// The proposed change payload (patch operations under `operations`).
    pub change: serde_json::Value,
    /// Graph revision the intent was drafted against (stale-awareness).
    pub base_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// What explicit promotion did with an intent draft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "detail", rename_all = "kebab-case")]
pub enum IntentOutcome {
    /// The change was applied to the Vistalith graph as a governed patch.
    AppliedToGraph { revision: u64 },
    /// The target is SDDK-owned: the semantic change proposal is recorded as
    /// an observation and must become SDDK-governed work through SDDK's own
    /// flow (SPEC-001 invariant 4; the chat transcript is not SDDK Decision
    /// Memory).
    RoutedToSddkGovernance { subject: SubjectRef },
    /// The proposal went through SDDK's capability gateway (SPK-012): the
    /// decision and receipt are durable in the `sddk-proposal-submitted`
    /// event this outcome accompanies.
    SubmittedToSddk {
        subject: SubjectRef,
        proposal: SubjectRef,
        receipt_id: Option<String>,
        decision: String,
    },
    /// The graph moved on since the draft; preview is stale, promotion denied.
    StaleBase { current_revision: u64 },
    /// Rejected locally by patch validation (unknown subject, etc.).
    RejectedLocally { reason: String },
    /// The user discarded the draft.
    Discarded {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentPromoted {
    pub intent: SubjectRef,
    pub outcome: IntentOutcome,
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
            EventPayload::ToolInvoked(_) => "tool-invoked",
            EventPayload::ThreadForked(_) => "thread-forked",
            EventPayload::AdvisoryRaised(_) => "advisory-raised",
            EventPayload::AgentDefined(_) => "agent-defined",
            EventPayload::SddkProposalSubmitted(_) => "sddk-proposal-submitted",
            EventPayload::UatCheckRecorded(_) => "uat-check-recorded",
            EventPayload::AgentRunFinished(_) => "agent-run-finished",
            EventPayload::FrameStarted(_) => "frame-started",
            EventPayload::FrameTurnCompleted(_) => "frame-turn-completed",
            EventPayload::FrameClosed(_) => "frame-closed",
            EventPayload::IntentDrafted(_) => "intent-drafted",
            EventPayload::IntentPromoted(_) => "intent-promoted",
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

    #[test]
    fn thread_forked_roundtrips() {
        let event = VEvent {
            event_id: Uuid::now_v7(),
            actor: Actor::new("user:ruben").unwrap(),
            timestamp: OffsetDateTime::parse(
                "2026-09-04T10:00:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
            subjects: vec![container("thread-2"), container("thread-1")],
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            trace_id: None,
            payload: EventPayload::ThreadForked(ThreadForked {
                fork: container("thread-2"),
                source: container("thread-1"),
                up_to_turn: 3,
                note: Some("explore cheaper model".to_owned()),
            }),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "thread-forked");
        assert_eq!(json["payload"]["up_to_turn"], 3);
        let back: VEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn old_logs_without_forked_of_still_parse() {
        let json = serde_json::json!({
            "type": "message-appended",
            "payload": {
                "thread": { "namespace": "agentic", "kind": "thread", "id": "t1" },
                "message": { "namespace": "agentic", "kind": "message", "id": "m1" },
                "role": "user",
                "content": "hello",
                "turn": 1
            }
        });
        let payload: EventPayload = serde_json::from_value(json).unwrap();
        match payload {
            EventPayload::MessageAppended(appended) => {
                assert!(appended.forked_of.is_none());
                assert_eq!(appended.content, "hello");
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }
}
