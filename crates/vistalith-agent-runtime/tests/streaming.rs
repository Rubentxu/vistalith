//! Streaming turns (slice 11): the streamed path emits deltas and finishes
//! with exactly the same durability as the non-streamed path — same events,
//! same order, same accounting.

use std::sync::Arc;

use vistalith_agent_runtime::{
    ConversationEngine, FakeProvider, FakeStep, GrantStore, ToolRegistry,
};
use vistalith_graph::GraphStore;

#[tokio::test]
async fn streamed_turn_emits_deltas_then_finishes_durably() {
    let mut store = GraphStore::new();
    let engine = ConversationEngine::new(FakeProvider::repeating(
        "alpha beta gamma delta epsilon zeta",
    ));

    let thread = engine.start_thread(&mut store, "streaming").unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let reply = engine
        .send_user_message_streaming(&mut store, &thread, "say the words", tx)
        .await
        .unwrap();

    // Deltas arrived before the reply returned (the engine awaits them).
    let mut deltas = Vec::new();
    while let Ok(Some(text)) =
        tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await
    {
        deltas.push(text);
    }
    assert!(deltas.len() >= 2, "fake provider chunks into >=2 deltas");
    assert_eq!(
        deltas.join(" ").split_whitespace().count(),
        6,
        "deltas concatenate to the full text: {deltas:?}"
    );

    // Durability identical to the non-streamed path.
    let node = store.graph().subject(&reply.message).unwrap();
    assert_eq!(
        node.properties["content"],
        serde_json::json!("alpha beta gamma delta epsilon zeta")
    );
    assert_eq!(node.properties["turn"], serde_json::json!(1));
    assert!(store.graph().subject(&thread).unwrap().properties["turns"]
        == serde_json::json!(1));

    // Replay of the streamed turn's log matches the live projection.
    let stored = store.to_log_json();
    let replayed = GraphStore::from_stored_json(&stored).unwrap();
    assert_eq!(replayed.digest(), store.digest());
}

#[tokio::test]
async fn streamed_tool_rounds_still_execute_and_durably_record() {
    let mut store = GraphStore::new();
    let grants = Arc::new(GrantStore::new());
    let engine = ConversationEngine::new(FakeProvider::steps(vec![
        FakeStep::ToolCall {
            name: "graph_search".to_owned(),
            arguments: serde_json::json!({ "query": "payment" }),
        },
        FakeStep::Text("search done".to_owned()),
    ]))
    .with_tools(ToolRegistry::native(Arc::clone(&grants)));

    // Seed the fixture so graph_search has something to find.
    let mut fixture = GraphStore::from_fixture_path(
        "../vistalith-graph/tests/fixtures/sample-world.json",
    )
    .unwrap();
    for event in store.log() {
        fixture.append(event.event.clone()).unwrap();
    }
    store = fixture;

    let thread = engine.start_thread(&mut store, "streaming tools").unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let reply = engine
        .send_user_message_streaming(&mut store, &thread, "find payments", tx)
        .await
        .unwrap();
    assert_eq!(reply.content, "search done");
    drop(rx); // the engine drained its own stream before returning

    // The tool round executed durably before the streamed text.
    let tool_items = store
        .graph()
        .subjects_of_kind(&vistalith_domain::SubjectKind::ToolCall)
        .count();
    assert_eq!(tool_items, 1, "tool call recorded");
    assert_eq!(reply.turn, 1);
}

#[tokio::test]
async fn tool_call_arriving_on_the_streamed_round_is_executed() {
    // A provider whose FINAL round streams a tool call: the engine must
    // execute it and continue rather than rendering it as prose.
    struct StreamingToolCaller {
        inner: FakeProvider,
    }
    impl vistalith_agent_runtime::ModelProvider for StreamingToolCaller {
        fn descriptor(&self) -> &vistalith_domain::ModelDescriptor {
            self.inner.descriptor()
        }

        async fn complete(
            &self,
            request: vistalith_agent_runtime::ModelRequest,
        ) -> Result<vistalith_agent_runtime::ModelResponse, vistalith_agent_runtime::ModelError>
        {
            self.inner.complete(request).await
        }

        async fn stream_complete(
            &self,
            request: vistalith_agent_runtime::ModelRequest,
        ) -> Result<vistalith_agent_runtime::ModelEventRx, vistalith_agent_runtime::ModelError>
        {
            let response = self.inner.next_public(&request)?;
            let (tx, rx) = tokio::sync::mpsc::channel(16);
            tokio::spawn(async move {
                let _ = tx
                    .send(Ok(vistalith_agent_runtime::ModelEvent::Delta {
                        text: String::new(),
                    }))
                    .await;
                let _ = tx
                    .send(Ok(vistalith_agent_runtime::ModelEvent::Finished {
                        content: response.content,
                        model: response.model,
                        usage: response.usage,
                        tool_calls: response.tool_calls,
                    }))
                    .await;
            });
            Ok(rx)
        }
    }

    // expose next_response for the shim
    trait NextPublic {
        fn next_public(
            &self,
            request: &vistalith_agent_runtime::ModelRequest,
        ) -> Result<vistalith_agent_runtime::ModelResponse, vistalith_agent_runtime::ModelError>;
    }
    impl NextPublic for FakeProvider {
        fn next_public(
            &self,
            request: &vistalith_agent_runtime::ModelRequest,
        ) -> Result<vistalith_agent_runtime::ModelResponse, vistalith_agent_runtime::ModelError>
        {
            self.next_response_for_tests(request)
        }
    }

    let mut store = GraphStore::new();
    let grants = Arc::new(GrantStore::new());
    let provider = StreamingToolCaller {
        inner: FakeProvider::steps(vec![
            FakeStep::ToolCall {
                name: "graph_search".to_owned(),
                arguments: serde_json::json!({ "query": "payment" }),
            },
            FakeStep::Text("done after streamed tool call".to_owned()),
        ]),
    };
    let engine =
        ConversationEngine::new(provider).with_tools(ToolRegistry::native(Arc::clone(&grants)));

    let thread = engine.start_thread(&mut store, "streamed tool").unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    let reply = engine
        .send_user_message_streaming(&mut store, &thread, "go", tx)
        .await
        .unwrap();
    assert_eq!(reply.content, "done after streamed tool call");
    assert_eq!(reply.turn, 1);
}
