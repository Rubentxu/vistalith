use vistalith_agent_runtime::{
    ChatMessage, ConversationEngine, ConversationError, FakeProvider, ModelRequest,
    RuntimeProvider,
};
use vistalith_domain::{MessageRole, Namespace, SubjectKind, SubjectRef};
use vistalith_graph::GraphStore;

fn thread_subject(store: &GraphStore) -> SubjectRef {
    store
        .graph()
        .subjects_of_kind(&SubjectKind::Thread)
        .next()
        .expect("thread subject exists")
        .subject
        .clone()
}

fn messages_of(store: &GraphStore, thread: &SubjectRef, role: MessageRole) -> Vec<(u64, String)> {
    store
        .graph()
        .children(thread)
        .into_iter()
        .filter(|n| n.properties.get("role").and_then(|r| r.as_str()) == Some(role_str(role)))
        .map(|n| {
            (
                n.properties
                    .get("turn")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0),
                n.properties
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect()
}

fn role_str(role: MessageRole) -> &'static str {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
        MessageRole::Tool => "tool",
    }
}

#[tokio::test]
async fn thread_start_is_durable_and_projected() {
    let mut store = GraphStore::new();
    let engine = ConversationEngine::new(FakeProvider::repeating("ok"));

    let thread = engine
        .start_thread(&mut store, "Payment modelling")
        .unwrap();

    assert!(thread.to_string().starts_with("agentic:thread:"));
    let node = store.graph().subject(&thread).unwrap();
    assert_eq!(
        node.properties.get("title").and_then(|v| v.as_str()),
        Some("Payment modelling")
    );
    assert_eq!(store.log().last().unwrap().event.kind(), "thread-started");
}

#[tokio::test]
async fn turn_is_fully_durable_user_reply_usage() {
    let mut store = GraphStore::new();
    let engine = ConversationEngine::new(FakeProvider::scripted(vec![
        "Turn one answer".to_owned(),
        "Turn two answer".to_owned(),
    ]));

    let thread = engine.start_thread(&mut store, "demo").unwrap();
    let first = engine
        .send_user_message(&mut store, &thread, "hello")
        .await
        .unwrap();
    let second = engine
        .send_user_message(&mut store, &thread, "again")
        .await
        .unwrap();

    assert_eq!(first.content, "Turn one answer");
    assert_eq!(second.content, "Turn two answer");
    assert_eq!(first.turn, 1);
    assert_eq!(second.turn, 2);

    // Typed items: roles and turns are structure, not flattened prose.
    let user = messages_of(&store, &thread, MessageRole::User);
    let assistant = messages_of(&store, &thread, MessageRole::Assistant);
    assert_eq!(user, vec![(1, "hello".to_owned()), (2, "again".to_owned())]);
    assert_eq!(
        assistant,
        vec![
            (1, "Turn one answer".to_owned()),
            (2, "Turn two answer".to_owned())
        ]
    );

    // Thread progress + usage are durable facts.
    let thread_node = store.graph().subject(&thread).unwrap();
    assert_eq!(
        thread_node.properties.get("turns").and_then(|v| v.as_u64()),
        Some(2)
    );
    assert!(thread_node.properties.contains_key("last_usage"));

    // The model is an observed SWG subject with a used_model edge.
    let model = SubjectRef::new(Namespace::Agentic, SubjectKind::Model, "fake/scripted-1").unwrap();
    assert!(store.graph().subject(&model).is_some());
    assert!(
        store
            .graph()
            .outgoing(&thread)
            .any(|f| f.relation.to == model && f.relation.kind.as_str() == "used_model")
    );

    // The event log holds the full story.
    let kinds: Vec<_> = store
        .log()
        .iter()
        .map(|e| e.event.kind().to_owned())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "thread-started",
            "message-appended",
            "message-appended",
            "turn-completed",
            "message-appended",
            "message-appended",
            "turn-completed",
        ]
    );
}

#[tokio::test]
async fn model_history_is_reconstructed_from_durable_state() {
    let mut store = GraphStore::new();
    let engine = ConversationEngine::new(FakeProvider::scripted(vec![
        "a1".to_owned(),
        "a2".to_owned(),
    ]));

    let thread = engine.start_thread(&mut store, "history").unwrap();
    engine
        .send_user_message(&mut store, &thread, "first question")
        .await
        .unwrap();
    engine
        .send_user_message(&mut store, &thread, "second question")
        .await
        .unwrap();

    let requests = engine.provider().recorded_requests();
    assert_eq!(requests.len(), 2);

    // Turn 1: just the prompt.
    assert_eq!(chat_contents(&requests[0]), vec!["first question"]);

    // Turn 2: prior user message, prior assistant reply, then the new prompt.
    assert_eq!(
        chat_contents(&requests[1]),
        vec!["first question", "a1", "second question"]
    );
    assert_eq!(
        requests[1].messages[1].role,
        MessageRole::Assistant,
        "history carries typed roles"
    );
}

fn chat_contents(request: &ModelRequest) -> Vec<&str> {
    request
        .messages
        .iter()
        .map(|m: &ChatMessage| m.content.as_str())
        .collect()
}

#[tokio::test]
async fn conversation_state_rebuilds_from_the_durable_log() {
    let mut store = GraphStore::new();
    let engine = ConversationEngine::new(FakeProvider::scripted(vec![
        "one".to_owned(),
        "two".to_owned(),
    ]));
    let thread = engine.start_thread(&mut store, "rebuild").unwrap();
    engine
        .send_user_message(&mut store, &thread, "q1")
        .await
        .unwrap();
    engine
        .send_user_message(&mut store, &thread, "q2")
        .await
        .unwrap();

    let rebuilt = GraphStore::from_stored_json(&store.to_log_json()).unwrap();
    assert_eq!(rebuilt.digest(), store.digest());

    let rebuilt_thread = thread_subject(&rebuilt);
    assert_eq!(
        messages_of(&rebuilt, &rebuilt_thread, MessageRole::Assistant).len(),
        2,
        "assistant items reconstruct from events alone"
    );
}

#[tokio::test]
async fn unknown_threads_and_empty_messages_are_rejected() {
    let mut store = GraphStore::new();
    let engine = ConversationEngine::new(FakeProvider::repeating("ok"));
    let ghost = SubjectRef::new(Namespace::Agentic, SubjectKind::Thread, "nope").unwrap();

    match engine.send_user_message(&mut store, &ghost, "hi").await {
        Err(ConversationError::UnknownThread(identity)) => {
            assert_eq!(identity, "agentic:thread:nope");
        }
        other => panic!("expected unknown thread, got {other:?}"),
    }

    let thread = engine.start_thread(&mut store, "t").unwrap();
    match engine.send_user_message(&mut store, &thread, "   ").await {
        Err(ConversationError::EmptyMessage) => {}
        other => panic!("expected empty message, got {other:?}"),
    }
}

#[tokio::test]
async fn runtime_provider_enum_dispatches() {
    let runtime = RuntimeProvider::Fake(FakeProvider::repeating("dispatched"));
    let response = runtime
        .complete(ModelRequest {
            model: runtime.descriptor().clone(),
            system: None,
            messages: vec![ChatMessage {
                role: MessageRole::User,
                content: "hi".to_owned(),
            }],
            max_tokens: None,
        })
        .await
        .unwrap();
    assert_eq!(response.content, "dispatched");
}
