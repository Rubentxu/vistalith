use vistalith_agent_runtime::{
    ChatMessage, ConversationEngine, FakeProvider, FakeStep, GraphSearchTool, IntentError,
    GrantStore, ModelRequest, NativeTool, PermissionDecision, Promotion, RuntimeProvider,
    ToolRegistry,
    discard_intent, draft_intent, promote_intent,
};
use vistalith_domain::{MessageRole, Namespace, SubjectKind, SubjectRef};
use vistalith_graph::GraphStore;

fn engine_with(tools: ToolRegistry) -> ConversationEngine<FakeProvider> {
    ConversationEngine::new(FakeProvider::steps(vec![
        FakeStep::ToolCall {
            name: "graph_search".to_owned(),
            arguments: serde_json::json!({ "query": "payment" }),
        },
        FakeStep::Text("The graph has one payment container.".to_owned()),
    ]))
    .with_tools(tools)
}

#[tokio::test]
async fn native_tool_round_trip_is_fully_durable() {
    let mut store =
        GraphStore::from_fixture_path("../vistalith-graph/tests/fixtures/sample-world.json")
            .unwrap();
    let engine = engine_with(ToolRegistry::native(std::sync::Arc::new(GrantStore::new())));

    let thread = engine.start_thread(&mut store, "tooling").unwrap();
    let reply = engine
        .send_user_message(&mut store, &thread, "what payment subjects exist?")
        .await
        .unwrap();

    assert_eq!(reply.content, "The graph has one payment container.");

    // The tool call is a typed item, not prose: agentic:tool-call subject with
    // structured args/output and a used_tool edge from the thread.
    let tool_calls: Vec<_> = store
        .graph()
        .subjects_of_kind(&SubjectKind::ToolCall)
        .collect();
    assert_eq!(tool_calls.len(), 1);
    let node = tool_calls[0];
    assert_eq!(
        node.properties.get("tool").and_then(|v| v.as_str()),
        Some("graph_search")
    );
    assert_eq!(
        node.properties
            .get("args")
            .and_then(|a| a.get("query"))
            .and_then(|q| q.as_str()),
        Some("payment")
    );
    assert_eq!(
        node.properties
            .get("output")
            .and_then(|o| o.get("count"))
            .and_then(|c| c.as_u64()),
        Some(2),
        "graph_search found the fixture's payment container + repository"
    );
    assert!(
        store
            .graph()
            .outgoing(&thread)
            .any(|f| f.relation.kind.as_str() == "used_tool" && f.relation.to == node.subject)
    );

    // The model received the tool output on its second round.
    let requests = engine.provider().recorded_requests();
    assert_eq!(requests.len(), 2);
    let tool_items: Vec<_> = requests[1]
        .messages
        .iter()
        .filter(|m| m.role == MessageRole::Tool)
        .collect();
    assert_eq!(tool_items.len(), 1);
    assert!(tool_items[0].content.contains("payment-service"));

    // Event log carries the typed story (after the 5 fixture events).
    let kinds: Vec<_> = store
        .log()
        .iter()
        .skip(5)
        .map(|e| e.event.kind().to_owned())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "thread-started",
            "message-appended",
            "tool-invoked",
            "message-appended",
            "message-appended",
            "turn-completed",
        ]
    );
}

#[tokio::test]
async fn graph_search_respects_filters() {
    let store =
        GraphStore::from_fixture_path("../vistalith-graph/tests/fixtures/sample-world.json")
            .unwrap();
    let tool = GraphSearchTool;

    let all = tool
        .execute(store.graph(), &serde_json::json!({ "query": "payment" }))
        .unwrap();
    assert_eq!(all["count"], 2); // container + repository

    let containers = tool
        .execute(
            store.graph(),
            &serde_json::json!({ "query": "payment", "kind": "container" }),
        )
        .unwrap();
    assert_eq!(containers["count"], 1);

    let bad = tool.execute(store.graph(), &serde_json::json!({}));
    assert!(bad.is_err(), "missing query must be rejected");
}

#[test]
fn write_tools_are_denied_by_the_registry() {
    let registry = ToolRegistry::native(std::sync::Arc::new(GrantStore::new()));
    assert_eq!(
        registry.permission("graph_search").unwrap(),
        PermissionDecision::Allow
    );
    assert!(matches!(
        registry.permission("file_delete"),
        Err(vistalith_agent_runtime::ToolError::UnknownTool(_))
    ));
}

#[tokio::test]
async fn intent_promotes_to_graph_patch_when_fresh() {
    let mut store =
        GraphStore::from_fixture_path("../vistalith-graph/tests/fixtures/sample-world.json")
            .unwrap();
    let actor = vistalith_domain::Actor::new("user:rubentxu").unwrap();
    let target = SubjectRef::parse("arch:container:payment-service").unwrap();

    // Gesture -> draft only (SPEC-006).
    let intent = draft_intent(
        &mut store,
        &target,
        "rename",
        serde_json::json!({
            "operations": [{
                "op": "upsert-subject",
                "subject": { "namespace": "arch", "kind": "container", "id": "payment-service" },
                "authority": "authoritative",
                "provenance": { "source": "user:rubentxu" },
                "properties": { "name": "Payments Service" }
            }]
        }),
        Some("canvas rename gesture".to_owned()),
        &actor,
    )
    .unwrap();

    let node = store.graph().subject(&intent).unwrap();
    assert_eq!(node.authority, vistalith_domain::AuthorityClass::Advisory);
    assert_eq!(
        node.properties.get("status").and_then(|v| v.as_str()),
        Some("draft")
    );
    // The intent proposes a change to its resolved subject.
    assert!(
        store.graph().outgoing(&intent).any(|f| {
            f.relation.kind.as_str() == "proposes_change_to" && f.relation.to == target
        })
    );

    // Explicit promotion applies the governed patch.
    let outcome = promote_intent(&mut store, &intent, &actor).unwrap();
    match outcome {
        // Draft event produced revision 6; the promoted patch produced 7.
        Promotion::Applied { revision } => assert_eq!(revision, 7),
        other => panic!("expected applied, got {other:?}"),
    }
    let renamed = store.graph().subject(&target).unwrap();
    assert_eq!(
        renamed.properties.get("name").and_then(|v| v.as_str()),
        Some("Payments Service")
    );
    assert_eq!(
        store
            .graph()
            .subject(&intent)
            .unwrap()
            .properties
            .get("status"),
        Some(&serde_json::json!("applied"))
    );
}

#[tokio::test]
async fn sddk_owned_targets_route_to_governance_instead_of_applying() {
    let mut store =
        GraphStore::from_fixture_path("../vistalith-graph/tests/fixtures/sample-world.json")
            .unwrap();
    let actor = vistalith_domain::Actor::new("agent:planner").unwrap();
    let target = SubjectRef::parse("sddk:work-item:TEST-MODEL-001").unwrap();

    let intent = draft_intent(
        &mut store,
        &target,
        "rename",
        serde_json::json!({
            "operations": [{
                "op": "upsert-subject",
                "subject": { "namespace": "sddk", "kind": "work-item", "id": "TEST-MODEL-001" },
                "authority": "authoritative",
                "provenance": { "source": "agent:planner" },
                "properties": { "title": "hijack" }
            }]
        }),
        None,
        &actor,
    )
    .unwrap();

    let outcome = promote_intent(&mut store, &intent, &actor).unwrap();
    match outcome {
        Promotion::RoutedToSddkGovernance { subject } => {
            assert_eq!(subject, target);
        }
        other => panic!("expected sddk governance route, got {other:?}"),
    }
    // The SDDK-owned subject is untouched; the intent records the route.
    assert_eq!(
        store
            .graph()
            .subject(&target)
            .unwrap()
            .properties
            .get("title")
            .and_then(|v| v.as_str()),
        Some("Model the payment service in the SWG")
    );
    assert_eq!(
        store
            .graph()
            .subject(&intent)
            .unwrap()
            .properties
            .get("status"),
        Some(&serde_json::json!("sddk-governed"))
    );
}

#[tokio::test]
async fn stale_drafts_cannot_be_promoted() {
    let mut store =
        GraphStore::from_fixture_path("../vistalith-graph/tests/fixtures/sample-world.json")
            .unwrap();
    let actor = vistalith_domain::Actor::new("user:rubentxu").unwrap();
    let target = SubjectRef::parse("arch:container:payment-service").unwrap();

    let intent = draft_intent(
        &mut store,
        &target,
        "rename",
        serde_json::json!({
            "operations": [{
                "op": "upsert-subject",
                "subject": { "namespace": "arch", "kind": "container", "id": "payment-service" },
                "authority": "authoritative",
                "provenance": { "source": "user:rubentxu" },
                "properties": { "name": "Later Name" }
            }]
        }),
        None,
        &actor,
    )
    .unwrap();

    // The graph moves on after the draft: base revision 6, now 7.
    store
        .propose_patch(vistalith_graph::GraphPatch {
            patch_id: vistalith_domain::PatchId::new("unrelated").unwrap(),
            base_revision: 6,
            proposed_by: actor.clone(),
            operations: vec![vistalith_domain::PatchOperation::UpsertSubject {
                subject: SubjectRef::new(Namespace::Visual, SubjectKind::Note, "n1").unwrap(),
                authority: vistalith_domain::AuthorityClass::Advisory,
                provenance: vistalith_domain::Provenance::new("user:rubentxu").unwrap(),
                properties: Default::default(),
            }],
        })
        .unwrap();

    let outcome = promote_intent(&mut store, &intent, &actor).unwrap();
    assert_eq!(
        outcome,
        Promotion::Stale {
            current_revision: 7,
            base_revision: 6
        }
    );
    assert_eq!(
        store
            .graph()
            .subject(&intent)
            .unwrap()
            .properties
            .get("status"),
        Some(&serde_json::json!("stale"))
    );
}

#[tokio::test]
async fn discard_records_without_executing() {
    let mut store =
        GraphStore::from_fixture_path("../vistalith-graph/tests/fixtures/sample-world.json")
            .unwrap();
    let actor = vistalith_domain::Actor::new("user:rubentxu").unwrap();
    let target = SubjectRef::parse("arch:container:payment-service").unwrap();

    let intent = draft_intent(
        &mut store,
        &target,
        "annotate",
        serde_json::json!({ "operations": [] }),
        None,
        &actor,
    )
    .unwrap();
    discard_intent(&mut store, &intent, Some("changed my mind".into()), &actor).unwrap();

    assert_eq!(
        store
            .graph()
            .subject(&intent)
            .unwrap()
            .properties
            .get("status"),
        Some(&serde_json::json!("discarded"))
    );
    // A discarded draft cannot be promoted anymore.
    assert!(matches!(
        promote_intent(&mut store, &intent, &actor),
        Err(IntentError::AlreadyResolved(_, _))
    ));
}

#[tokio::test]
async fn drafts_require_existing_targets() {
    let mut store = GraphStore::new();
    let actor = vistalith_domain::Actor::new("user:rubentxu").unwrap();
    let ghost = SubjectRef::parse("arch:container:ghost").unwrap();
    assert!(matches!(
        draft_intent(
            &mut store,
            &ghost,
            "rename",
            serde_json::json!({}),
            None,
            &actor
        ),
        Err(IntentError::UnknownTarget(_))
    ));
}

#[tokio::test]
async fn runtime_provider_contract_carries_tools() {
    let runtime = RuntimeProvider::Fake(FakeProvider::repeating("ok"));
    let request = ModelRequest {
        model: runtime.descriptor().clone(),
        system: None,
        messages: vec![ChatMessage {
            role: MessageRole::User,
            content: "hi".to_owned(),
        }],
        max_tokens: None,
        tools: vec![vistalith_agent_runtime::ToolContract {
            name: "graph_search".to_owned(),
            description: "search".to_owned(),
            parameters: serde_json::json!({ "type": "object" }),
        }],
    };
    let response = runtime.complete(request).await.unwrap();
    assert_eq!(response.content, "ok");
    assert!(!response.is_tool_call());
}
