//! Frames: bounded execution contexts (`graph/PATTERNS-VIEWS-FRAMES.md`,
//! slice 8). A frame declares a goal, the semantic subjects its context is
//! built from, the permitted tools, and hard budgets (turns, tokens). Turns
//! run inside a frame-owned thread with a restricted catalog; usage is
//! durable (`frame-turn-completed`), and closed frames accept no further
//! turns. Frames are Vistalith-owned agentic constructs — they never become
//! a second SDDK workflow authority.

use std::collections::BTreeMap;

use thiserror::Error;
use uuid::Uuid;
use vistalith_domain::{
    Actor, EventPayload, FrameClosed, FrameOutcome, FrameStarted, FrameTurnCompleted,
    ModelUsage, Namespace, RelationDeclared, RelationFact, RelationKind, RelationRef, SubjectKind,
    SubjectRef, ThreadStarted, VEvent,
};
use vistalith_graph::GraphStore;

use crate::conversation::ConversationEngine;
use crate::provider::ModelProvider;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("frame `{0}` is already closed")]
    Closed(String),
    #[error("frame `{0}` exhausted its turn budget ({1} turns)")]
    TurnsExhausted(String, u32),
    #[error("frame `{0}` exhausted its token budget ({1} tokens)")]
    BudgetExhausted(String, u64),
    #[error("unknown frame `{0}`")]
    UnknownFrame(String),
    #[error("frame `{0}` has no thread (corrupt frame?)")]
    NoThread(String),
    #[error("bounded subject `{0}` does not exist")]
    UnknownSubject(String),
    #[error(transparent)]
    Store(#[from] vistalith_graph::StoreError),
    #[error(transparent)]
    Conversation(#[from] crate::conversation::ConversationError),
}

/// Declarative frame spec (`graph/PATTERNS-VIEWS-FRAMES.md` subset durable
/// in v1: goal, subjects, permitted tools, budgets; branch/ref and expected
/// structured outputs are follow-ups).
#[derive(Debug, Clone)]
pub struct FrameSpec {
    pub goal: String,
    pub agent: Option<SubjectRef>,
    pub subjects: Vec<SubjectRef>,
    pub permitted_tools: Vec<String>,
    pub max_turns: u32,
    pub token_budget: u64,
}

/// Result of one bounded frame turn.
#[derive(Debug, Clone)]
pub struct FrameTurnReport {
    pub frame: SubjectRef,
    pub thread: SubjectRef,
    pub turn: u64,
    pub usage: ModelUsage,
    /// Frame-level accounting after the turn.
    pub turns_used: u64,
    pub used_tokens: u64,
    /// Set when this turn tripped a budget and the frame auto-closed.
    pub auto_closed: Option<FrameOutcome>,
}

fn frame_actor() -> Actor {
    Actor::new("system:frames").expect("static actor")
}

fn frame_event(payload: EventPayload) -> VEvent {
    VEvent {
        event_id: Uuid::now_v7(),
        actor: frame_actor(),
        timestamp: time::OffsetDateTime::now_utc(),
        subjects: Vec::new(),
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        trace_id: None,
        payload,
    }
}

/// Registers a Vistalith agent (`agentic/AGENTS-DELEGATION.md`).
pub fn define_agent(
    store: &mut GraphStore,
    role: impl Into<String>,
    instructions: impl Into<String>,
    model: Option<vistalith_domain::ModelDescriptor>,
    tools: Vec<String>,
    budget_turns: Option<u32>,
    expected_outputs: Vec<String>,
) -> Result<SubjectRef, FrameError> {
    let agent = SubjectRef::new(
        Namespace::Agentic,
        SubjectKind::Agent,
        Uuid::now_v7().to_string(),
    )
    .expect("generated agent id is valid");
    store.append(frame_event(EventPayload::AgentDefined(
        vistalith_domain::AgentDefined {
            agent: agent.clone(),
            role: role.into(),
            instructions: instructions.into(),
            model,
            tools,
            budget_turns,
            expected_outputs,
        },
    )))?;
    Ok(agent)
}

/// Starts a frame: the frame subject, a frame-owned thread, and the links
/// (`delegated_to` the agent, `mentions` the bounded subjects).
pub fn start_frame(store: &mut GraphStore, spec: FrameSpec) -> Result<SubjectRef, FrameError> {
    for subject in &spec.subjects {
        if store.graph().subject(subject).is_none() {
            return Err(FrameError::UnknownSubject(subject.to_string()));
        }
    }
    if let Some(agent) = &spec.agent
        && store.graph().subject(agent).is_none()
    {
        return Err(FrameError::UnknownSubject(agent.to_string()));
    }
    let frame = SubjectRef::new(
        Namespace::Agentic,
        SubjectKind::Frame,
        Uuid::now_v7().to_string(),
    )
    .expect("generated frame id is valid");
    store.append(frame_event(EventPayload::FrameStarted(FrameStarted {
        frame: frame.clone(),
        goal: spec.goal.clone(),
        agent: spec.agent.clone(),
        subjects: spec.subjects.clone(),
        permitted_tools: spec.permitted_tools.clone(),
        max_turns: spec.max_turns,
        token_budget: spec.token_budget,
    })))?;

    // The frame owns a thread; its turns are ordinary durable thread turns.
    let thread = SubjectRef::new(
        Namespace::Agentic,
        SubjectKind::Thread,
        Uuid::now_v7().to_string(),
    )
    .expect("generated frame thread id is valid");
    store.append(frame_event(EventPayload::ThreadStarted(ThreadStarted {
        thread: thread.clone(),
        title: format!("frame: {}", spec.goal),
    })))?;
    store.append(frame_event(EventPayload::RelationDeclared(
        RelationDeclared {
            fact: RelationFact {
                relation: RelationRef::new(
                    frame.clone(),
                    RelationKind::Contains,
                    thread.clone(),
                )
                .expect("frame and thread are distinct"),
                authority: vistalith_domain::AuthorityClass::Authoritative,
                provenance: vistalith_domain::Provenance::new("system:frames")
                    .expect("static provenance"),
            },
        },
    )))?;
    Ok(frame)
}

fn frame_properties(
    store: &GraphStore,
    frame: &SubjectRef,
) -> Result<BTreeMap<String, serde_json::Value>, FrameError> {
    store
        .graph()
        .subject(frame)
        .map(|node| node.properties.clone())
        .ok_or_else(|| FrameError::UnknownFrame(frame.to_string()))
}

fn require_open(
    properties: &BTreeMap<String, serde_json::Value>,
    frame: &SubjectRef,
) -> Result<(), FrameError> {
    if properties.get("status").and_then(|v| v.as_str()) != Some("open") {
        return Err(FrameError::Closed(frame.to_string()));
    }
    Ok(())
}

/// The frame-owned thread (via the `contains` edge).
pub fn frame_thread(store: &GraphStore, frame: &SubjectRef) -> Result<SubjectRef, FrameError> {
    store
        .graph()
        .outgoing(frame)
        .find(|fact| fact.relation.kind == RelationKind::Contains)
        .map(|fact| fact.relation.to.clone())
        .ok_or_else(|| FrameError::NoThread(frame.to_string()))
}

/// Starts a frame delegated to a defined agent: the agent's instructions,
/// tools and budget drive the frame (AGENTS-DELEGATION.md). Returns both
/// subjects.
pub fn start_agent_frame(
    store: &mut GraphStore,
    agent: &SubjectRef,
    goal: String,
    subjects: Vec<SubjectRef>,
    max_turns: u32,
    token_budget: u64,
) -> Result<(SubjectRef, Vec<String>), FrameError> {
    let node = store
        .graph()
        .subject(agent)
        .ok_or_else(|| FrameError::UnknownSubject(agent.to_string()))?;
    let role = node
        .properties
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("agent");
    let instructions = node
        .properties
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tools: Vec<String> = node
        .properties
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|t| t.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let agent_budget = node.properties.get("budget_turns").and_then(|v| v.as_u64());
    let effective_turns = agent_budget
        .map(|budget| budget.min(max_turns as u64) as u32)
        .unwrap_or(max_turns);

    let goal = format!("{goal} [agent: {role}] instructions: {instructions}");
    let frame = start_frame(
        store,
        FrameSpec {
            goal,
            agent: Some(agent.clone()),
            subjects,
            permitted_tools: tools,
            max_turns: effective_turns.max(1),
            token_budget,
        },
    )?;
    let frame_subject = frame;
    let permitted = store
        .graph()
        .subject(&frame_subject)
        .and_then(|node| {
            node.properties
                .get("permitted_tools")
                .and_then(|v| v.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|t| t.as_str().map(str::to_owned))
                        .collect()
                })
        })
        .unwrap_or_default();
    Ok((frame_subject, permitted))
}

/// Records a finished agent run: structured outputs as a durable
/// `agent-run-finished` event projected into the SWG with
/// contributes_to/executed_by traceability edges.
pub fn finish_agent_run(
    store: &mut GraphStore,
    frame: &SubjectRef,
    agent: &SubjectRef,
    status: impl Into<String>,
    findings: Vec<String>,
    risks: Vec<String>,
    assumptions: Vec<String>,
) -> Result<SubjectRef, FrameError> {
    if store.graph().subject(frame).is_none() {
        return Err(FrameError::UnknownFrame(frame.to_string()));
    }
    let run = SubjectRef::new(
        Namespace::Agentic,
        SubjectKind::WorkflowRun,
        Uuid::now_v7().to_string(),
    )
    .expect("generated run id is valid");
    store.append(frame_event(EventPayload::AgentRunFinished(
        vistalith_domain::AgentRunFinished {
            run: run.clone(),
            agent: agent.clone(),
            frame: frame.clone(),
            findings,
            risks,
            assumptions,
            status: status.into(),
        },
    )))?;
    Ok(run)
}

/// The frame's system prompt: goal, bounds and the semantic context view of
/// the bounded subjects (SPEC-005 view as agent context, with provenance
/// available through the same view API).
pub fn frame_system_prompt(store: &GraphStore, frame: &SubjectRef) -> Result<String, FrameError> {
    let properties = frame_properties(store, frame)?;
    let goal = properties
        .get("goal")
        .and_then(|v| v.as_str())
        .unwrap_or("(no goal)");
    let max_turns = properties
        .get("max_turns")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let token_budget = properties
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let bounded: Vec<String> = store
        .graph()
        .outgoing(frame)
        .filter(|fact| fact.relation.kind == RelationKind::Mentions)
        .map(|fact| fact.relation.to.to_string())
        .collect();
    Ok(format!(
        "You are operating inside Vistalith frame `{frame}`.\n\
         Goal: {goal}\n\
         Bounded semantic subjects: {}.\n\
         Hard bounds: at most {max_turns} turns, {token_budget} tokens.",
        if bounded.is_empty() {
            "(none)".to_owned()
        } else {
            bounded.join(", ")
        },
    ))
}

/// Runs one turn inside the frame. The caller supplies the engine already
/// built with the frame-restricted catalog (`ToolRegistry::restricted_to`)
/// and [`frame_system_prompt`]. Guards run before the turn; budgets close
/// the frame automatically after it.
#[allow(clippy::too_many_arguments)]
pub async fn run_frame_turn<P: ModelProvider + Sync>(
    store: &mut GraphStore,
    frame: &SubjectRef,
    engine: &ConversationEngine<P>,
    content: impl Into<String>,
) -> Result<FrameTurnReport, FrameError> {
    let properties = frame_properties(store, frame)?;
    require_open(&properties, frame)?;
    let frame_id = frame.to_string();
    let turns_used = properties
        .get("turns")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let max_turns = properties
        .get("max_turns")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let used_tokens = properties
        .get("used_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let token_budget = properties
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if turns_used >= max_turns as u64 {
        close_frame(store, frame, FrameOutcome::TurnsExhausted, None)?;
        return Err(FrameError::TurnsExhausted(frame_id, max_turns));
    }
    if token_budget > 0 && used_tokens >= token_budget {
        close_frame(store, frame, FrameOutcome::BudgetExhausted, None)?;
        return Err(FrameError::BudgetExhausted(frame_id, token_budget));
    }

    let thread = frame_thread(store, frame)?;
    let reply = engine.send_user_message(store, &thread, content).await?;

    store.append(frame_event(EventPayload::FrameTurnCompleted(
        FrameTurnCompleted {
            frame: frame.clone(),
            turn: reply.turn,
            model: reply.model.clone(),
            usage: reply.usage,
        },
    )))?;

    // Re-read accounting after the turn (the projection accumulated usage).
    let properties = frame_properties(store, frame)?;
    let turns_used = properties
        .get("turns")
        .and_then(|v| v.as_u64())
        .unwrap_or(reply.turn);
    let used_tokens = properties
        .get("used_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let mut auto_closed = None;
    if token_budget > 0 && used_tokens >= token_budget {
        close_frame(store, frame, FrameOutcome::BudgetExhausted, None)?;
        auto_closed = Some(FrameOutcome::BudgetExhausted);
    } else if turns_used >= max_turns as u64 {
        close_frame(store, frame, FrameOutcome::TurnsExhausted, None)?;
        auto_closed = Some(FrameOutcome::TurnsExhausted);
    }

    Ok(FrameTurnReport {
        frame: frame.clone(),
        thread,
        turn: reply.turn,
        usage: reply.usage,
        turns_used,
        used_tokens,
        auto_closed,
    })
}

/// Closes a frame explicitly. Idempotent-unfriendly on purpose: closing a
/// closed frame is an error (the durable outcome of the first close wins).
pub fn close_frame(
    store: &mut GraphStore,
    frame: &SubjectRef,
    outcome: FrameOutcome,
    summary: Option<String>,
) -> Result<(), FrameError> {
    let properties = frame_properties(store, frame)?;
    require_open(&properties, frame)?;
    store.append(frame_event(EventPayload::FrameClosed(FrameClosed {
        frame: frame.clone(),
        outcome,
        summary,
    })))?;
    Ok(())
}
