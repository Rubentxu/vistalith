//! Fork semantics (SPEC-011, slice 5): forking a thread at a turn boundary
//! copies durable items with `forked_of` bindings, links back via
//! `forked_from`, and the fork is a fully usable thread (it can continue
//! the conversation from where the source stopped).

use vistalith_agent_runtime::{ConversationEngine, FakeProvider, FakeStep};
use vistalith_domain::{MessageRole, Namespace, RelationKind, SubjectKind, SubjectRef};
use vistalith_graph::GraphStore;

fn fork_subject(store: &GraphStore, fork: &SubjectRef, kind: SubjectKind) -> Vec<SubjectRef> {
    store
        .graph()
        .children(fork)
        .into_iter()
        .filter(|n| n.subject.kind() == &kind)
        .map(|n| n.subject.clone())
        .collect()
}

#[tokio::test]
async fn fork_copies_items_up_to_the_requested_turn() {
    let mut store = GraphStore::new();
    let engine = ConversationEngine::new(FakeProvider::steps(vec![
        FakeStep::Text("first answer".to_owned()),
        FakeStep::Text("second answer".to_owned()),
    ]));

    let source = engine.start_thread(&mut store, "payments").unwrap();
    engine
        .send_user_message(&mut store, &source, "question one")
        .await
        .unwrap();
    engine
        .send_user_message(&mut store, &source, "question two")
        .await
        .unwrap();

    let forked = engine
        .fork_thread(&mut store, &source, Some(1), Some("cheaper model?".to_owned()))
        .unwrap();
    assert_eq!(forked.up_to_turn, 1);
    // Turn 1 = user + assistant; the fork thread event itself is not counted.
    assert_eq!(forked.copied_events, 3);

    let fork = &forked.fork;
    let fork_node = store.graph().subject(fork).unwrap();
    assert_eq!(
        fork_node.properties.get("title").and_then(|v| v.as_str()),
        Some("payments (fork ≤ turn 1)"),
        "the fork title derives from the source during projection"
    );
    assert_eq!(fork_node.properties.get("turns"), Some(&serde_json::json!(1)));

    // Two messages copied, each bound to its original (SPEC-011 bindings).
    let copied_messages = fork_subject(&store, fork, SubjectKind::Message);
    assert_eq!(copied_messages.len(), 2);
    for copied in &copied_messages {
        let node = store.graph().subject(copied).unwrap();
        let original = node.properties.get("forked_of").and_then(|v| v.as_str());
        let bound = original
            .and_then(|raw| SubjectRef::parse(raw).ok())
            .expect("forked_of is a valid SubjectRef");
        let original_node = store.graph().subject(&bound).expect("original exists");
        assert_eq!(
            node.properties.get("content"),
            original_node.properties.get("content"),
            "the copy preserves the item's content"
        );
    }

    // The fork links back to the source.
    let link = store
        .graph()
        .relations_of_kind(&RelationKind::ForkedFrom)
        .find(|f| &f.relation.from == fork)
        .expect("forked_from relation");
    assert_eq!(&link.relation.to, &source);
}

#[tokio::test]
async fn fork_defaults_to_the_latest_turn_and_can_continue_the_conversation() {
    let mut store = GraphStore::new();
    let engine = ConversationEngine::new(FakeProvider::steps(vec![
        FakeStep::Text("answer".to_owned()),
        FakeStep::Text("answer again".to_owned()),
    ]));

    let source = engine.start_thread(&mut store, "explore").unwrap();
    engine
        .send_user_message(&mut store, &source, "start here")
        .await
        .unwrap();

    // No explicit turn: fork at the latest turn.
    let forked = engine.fork_thread(&mut store, &source, None, None).unwrap();
    assert_eq!(forked.up_to_turn, 1);

    // The fork is a live thread: the next turn continues at turn 2.
    let reply = engine
        .send_user_message(&mut store, &forked.fork, "and from here?")
        .await
        .unwrap();
    assert_eq!(reply.turn, 2);
    assert_eq!(reply.content, "answer again");

    // The source is untouched by the fork's continuation.
    let source_node = store.graph().subject(&source).unwrap();
    assert_eq!(source_node.properties.get("turns"), Some(&serde_json::json!(1)));
}

#[tokio::test]
async fn fork_of_unknown_thread_fails_without_appending() {
    let mut store = GraphStore::new();
    let engine = ConversationEngine::new(FakeProvider::repeating("ok"));
    let ghost = SubjectRef::new(
        Namespace::Agentic,
        SubjectKind::Thread,
        "does-not-exist".to_owned(),
    )
    .unwrap();
    let err = engine.fork_thread(&mut store, &ghost, None, None).unwrap_err();
    assert!(matches!(err, vistalith_agent_runtime::ConversationError::UnknownThread(_)));
    assert!(store.log().is_empty(), "failed forks append nothing");
}

#[tokio::test]
async fn replaying_a_forked_log_reproduces_the_fork() {
    let mut store = GraphStore::new();
    let engine = ConversationEngine::new(FakeProvider::repeating("hello"));
    let source = engine.start_thread(&mut store, "durable").unwrap();
    engine
        .send_user_message(&mut store, &source, "hi")
        .await
        .unwrap();
    let forked = engine.fork_thread(&mut store, &source, None, None).unwrap();

    let stored = store.to_log_json();
    let rebuilt = GraphStore::from_stored_json(&stored).unwrap();
    assert_eq!(rebuilt.digest(), store.digest());
    let fork_node = rebuilt.graph().subject(&forked.fork).unwrap();
    assert_eq!(
        fork_node.properties.get("forked_from"),
        Some(&serde_json::json!(source.to_string()))
    );
    let _ = MessageRole::User; // keep the role import honest if assertions evolve
}
