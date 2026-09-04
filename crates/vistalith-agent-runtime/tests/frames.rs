//! Frame invariants (slice 8, `graph/PATTERNS-VIEWS-FRAMES.md`): bounded
//! execution contexts — turns run inside a frame-owned thread with a
//! restricted catalog, usage is durable, and budgets close the frame.

use std::sync::Arc;

use vistalith_agent_runtime::{
    ConversationEngine, FakeProvider, FrameError, FrameOutcome, FrameSpec, FrameTurnReport,
    GrantStore, ToolRegistry, close_frame, frame_system_prompt, frame_thread, run_frame_turn,
    start_frame,
};
use vistalith_domain::{Namespace, SubjectKind, SubjectRef};
use vistalith_graph::GraphStore;

fn container(id: &str) -> SubjectRef {
    SubjectRef::new(Namespace::Arch, SubjectKind::Container, id.to_owned()).unwrap()
}

fn define(store: &mut GraphStore, subject: SubjectRef) {
    store
        .append(vistalith_domain::VEvent {
            event_id: uuid::Uuid::now_v7(),
            actor: vistalith_domain::Actor::new("user:ruben").unwrap(),
            timestamp: time::OffsetDateTime::now_utc(),
            subjects: vec![subject.clone()],
            correlation_id: uuid::Uuid::now_v7(),
            causation_id: None,
            trace_id: None,
            payload: vistalith_domain::EventPayload::SubjectDefined(
                vistalith_domain::SubjectDefined {
                    subject,
                    authority: vistalith_domain::AuthorityClass::Authoritative,
                    provenance: vistalith_domain::Provenance::new("user:ruben").unwrap(),
                    properties: std::collections::BTreeMap::new(),
                },
            ),
        })
        .unwrap();
}

fn spec(goal: &str, subjects: Vec<SubjectRef>, max_turns: u32, token_budget: u64) -> FrameSpec {
    FrameSpec {
        goal: goal.to_owned(),
        agent: None,
        subjects,
        permitted_tools: Vec::new(),
        max_turns,
        token_budget,
    }
}

#[tokio::test]
async fn frame_lifecycle_is_durable_and_turns_accumulate_usage() {
    let mut store = GraphStore::new();
    define(&mut store, container("payment-service"));

    let frame = start_frame(
        &mut store,
        spec("analyse payment service", vec![container("payment-service")], 3, 10_000),
    )
    .unwrap();

    // The frame owns a thread and mentions its bounded subjects.
    let thread = frame_thread(&store, &frame).unwrap();
    let node = store.graph().subject(&frame).unwrap();
    assert_eq!(node.properties["status"], serde_json::json!("open"));
    assert_eq!(node.properties["turns"], serde_json::json!(0));
    assert!(store
        .graph()
        .outgoing(&frame)
        .any(|f| f.relation.kind == vistalith_domain::RelationKind::Mentions
            && f.relation.to == container("payment-service")));

    // The system prompt states goal and bounds (context for the model).
    let prompt = frame_system_prompt(&store, &frame).unwrap();
    assert!(prompt.contains("analyse payment service"));
    assert!(prompt.contains("payment-service"));
    assert!(prompt.contains("3 turns"));

    // Two turns inside the frame, usage accumulated at frame level.
    let grants = Arc::new(GrantStore::new());
    let catalog = ToolRegistry::native(Arc::clone(&grants));
    let engine = ConversationEngine::new(FakeProvider::repeating("frame reply")).with_tools(
        catalog.restricted_to(&[]),
    );

    let first = run_frame_turn(&mut store, &frame, &engine, "turn one")
        .await
        .unwrap();
    let FrameTurnReport { turns_used, used_tokens, auto_closed, .. } = &first;
    assert_eq!(*turns_used, 1);
    assert!(*used_tokens > 0);
    assert!(auto_closed.is_none());

    run_frame_turn(&mut store, &frame, &engine, "turn two").await.unwrap();
    let node = store.graph().subject(&frame).unwrap();
    assert_eq!(node.properties["turns"], serde_json::json!(2));
    let tokens_after_two = node.properties["used_tokens"].as_u64().unwrap();
    assert!(tokens_after_two > 0);

    // The frame thread carries the durable conversation.
    let messages = store.graph().children(&thread);
    assert_eq!(messages.len(), 4, "two turns = user + assistant each");

    // Replay of the stored log reproduces the frame state exactly.
    let stored = store.to_log_json();
    let replayed = GraphStore::from_stored_json(&stored).unwrap();
    assert_eq!(
        replayed.graph().subject(&frame).unwrap().properties["used_tokens"],
        serde_json::json!(tokens_after_two)
    );
}

#[tokio::test]
async fn turn_budget_closes_the_frame_automatically() {
    let mut store = GraphStore::new();
    define(&mut store, container("svc"));
    let frame = start_frame(&mut store, spec("one shot", vec![container("svc")], 1, 10_000))
        .unwrap();
    let grants = Arc::new(GrantStore::new());
    let engine = ConversationEngine::new(FakeProvider::repeating("ok"))
        .with_tools(ToolRegistry::native(Arc::clone(&grants)).restricted_to(&[]));

    let first = run_frame_turn(&mut store, &frame, &engine, "the only turn")
        .await
        .unwrap();
    assert_eq!(first.auto_closed, Some(FrameOutcome::TurnsExhausted));

    let node = store.graph().subject(&frame).unwrap();
    assert_eq!(node.properties["status"], serde_json::json!("turns-exhausted"));

    // Further turns are refused without mutating anything: the frame is now
    // closed, so the closed-frame guard fires first.
    let turns_before = store.log().len();
    let err = run_frame_turn(&mut store, &frame, &engine, "one more")
        .await
        .unwrap_err();
    assert!(matches!(err, FrameError::Closed(_)));
    assert_eq!(store.log().len(), turns_before, "refused turns append nothing");
}

#[tokio::test]
async fn token_budget_closes_the_frame_after_a_heavy_turn() {
    let mut store = GraphStore::new();
    define(&mut store, container("svc"));
    // FakeProvider usage: 8 output tokens + ~4 tokens/char input. A budget
    // of 30 is trippable by a single modest turn.
    let frame = start_frame(&mut store, spec("bounded", vec![container("svc")], 10, 30))
        .unwrap();
    let grants = Arc::new(GrantStore::new());
    let engine = ConversationEngine::new(FakeProvider::repeating("ok"))
        .with_tools(ToolRegistry::native(Arc::clone(&grants)).restricted_to(&[]));

    let long_prompt = "a turn with considerable length so the deterministic fake provider                        token accounting crosses the small thirty token budget of this frame";
    let outcome = run_frame_turn(&mut store, &frame, &engine, long_prompt)
        .await
        .unwrap();
    assert_eq!(outcome.auto_closed, Some(FrameOutcome::BudgetExhausted));
    let node = store.graph().subject(&frame).unwrap();
    assert_eq!(node.properties["status"], serde_json::json!("budget-exhausted"));
    assert!(outcome.used_tokens >= 30);
}

#[tokio::test]
async fn explicit_close_wins_and_closed_frames_refuse_turns() {
    let mut store = GraphStore::new();
    define(&mut store, container("svc"));
    let frame = start_frame(&mut store, spec("explore", vec![container("svc")], 5, 10_000))
        .unwrap();
    let grants = Arc::new(GrantStore::new());
    let engine = ConversationEngine::new(FakeProvider::repeating("ok"))
        .with_tools(ToolRegistry::native(Arc::clone(&grants)).restricted_to(&[]));

    close_frame(
        &mut store,
        &frame,
        FrameOutcome::Completed,
        Some("findings recorded".to_owned()),
    )
    .unwrap();
    let node = store.graph().subject(&frame).unwrap();
    assert_eq!(node.properties["status"], serde_json::json!("completed"));
    assert_eq!(
        node.properties["summary"],
        serde_json::json!("findings recorded")
    );

    let err = run_frame_turn(&mut store, &frame, &engine, "late turn")
        .await
        .unwrap_err();
    assert!(matches!(err, FrameError::Closed(_)));

    // Double close is refused too: the durable outcome of the first wins.
    assert!(close_frame(&mut store, &frame, FrameOutcome::Aborted, None).is_err());
}

#[test]
fn frames_validate_their_bounded_subjects() {
    let mut store = GraphStore::new();
    let ghost = SubjectRef::new(Namespace::Arch, SubjectKind::Container, "ghost".to_owned()).unwrap();
    let err = start_frame(&mut store, spec("bad", vec![ghost], 1, 10_000)).unwrap_err();
    assert!(matches!(err, FrameError::UnknownSubject(_)));
    assert!(store.log().is_empty(), "invalid frames append nothing");
}

#[test]
fn restricted_catalog_is_the_intersection_and_shares_grants() {
    let grants = Arc::new(GrantStore::new());
    let catalog = ToolRegistry::native(Arc::clone(&grants));
    let restricted = catalog.restricted_to(&["mcp_echo_echo".to_owned()]);
    assert!(restricted.get("graph_search").is_none());
    assert!(restricted.get("mcp_echo_echo").is_none(), "not in the base catalog");
    // Same grant store: a grant made outside still governs inside.
    assert!(Arc::ptr_eq(&catalog.grants(), &restricted.grants()));
}
