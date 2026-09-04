use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use vistalith_server::{AppState, router};

async fn call(app: Router, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = app.oneshot(request).await.expect("in-memory service");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("JSON body")
    };
    (status, json)
}

#[tokio::test]
async fn conversation_flows_over_http() {
    let app = router(AppState::new(vistalith_graph::GraphStore::new()));

    // Start a thread.
    let (status, body) = call(
        app.clone(),
        Request::post("/threads")
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "title": "Slice-3 chat" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let thread = body["thread"].as_str().expect("thread identity").to_owned();
    assert!(thread.starts_with("agentic:thread:"));

    // Send one message; the fake provider answers.
    let (status, reply) = call(
        app.clone(),
        Request::post(format!(
            "/threads/{}/messages",
            thread.rsplit(':').next().unwrap()
        ))
        .header("content-type", "application/json")
        .body(Body::from(r#"{ "content": "hello vistalith" }"#))
        .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["turn"], 1);
    assert!(reply["content"].as_str().unwrap().contains("fake provider"));
    assert!(reply["usage"]["total_tokens"].as_u64().unwrap() > 0);

    // The thread lists its typed items.
    let (status, thread_view) = call(
        app.clone(),
        Request::get(format!("/threads/{}", thread.rsplit(':').next().unwrap()))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let messages = thread_view["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");

    // Threads are listed with their durable progress.
    let (status, listing) = call(app, Request::get("/threads").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["threads"].as_array().unwrap().len(), 1);
    assert_eq!(listing["threads"][0]["turns"], 1);
}

#[tokio::test]
async fn unknown_thread_returns_404() {
    let app = router(AppState::empty());
    let (status, body) = call(
        app,
        Request::post("/threads/does-not-exist/messages")
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "content": "hi" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().unwrap().contains("does not exist"));
}

#[tokio::test]
async fn c4_view_projects_architecture_subjects() {
    // Seed the standard fixture: one container, its repository and an SDDK
    // work-item observation.
    let store = vistalith_graph::GraphStore::from_fixture_path(
        "../vistalith-graph/tests/fixtures/sample-world.json",
    )
    .expect("fixture replays");
    let app = router(AppState::new(store));

    let (status, view) = call(app, Request::get("/views/c4").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["revision"], 5);
    // Only architecture-family subjects are C4 elements: the container is,
    // the code repository and the sddk work-item are not.
    assert_eq!(view["containers"].as_array().unwrap().len(), 1);
    assert_eq!(
        view["containers"][0]["identity"],
        "arch:container:payment-service"
    );
    assert_eq!(view["containers"][0]["name"], "Payment Service");
    assert_eq!(view["systems"].as_array().unwrap().len(), 0);
    // No relation between two C4 elements in this fixture.
    assert_eq!(view["relationships"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn visual_intent_full_lifecycle_over_http() {
    let store = vistalith_graph::GraphStore::from_fixture_path(
        "../vistalith-graph/tests/fixtures/sample-world.json",
    )
    .expect("fixture replays");
    let app = router(AppState::new(store));

    // Gesture -> draft (201), base revision is the draft's own revision (6).
    let (status, draft) = call(
        app.clone(),
        Request::post("/intents")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "target": "arch:container:payment-service",
                    "gesture": "rename",
                    "actor": "user:rubentxu",
                    "change": { "operations": [{
                        "op": "upsert-subject",
                        "subject": { "namespace": "arch", "kind": "container", "id": "payment-service" },
                        "authority": "authoritative",
                        "provenance": { "source": "user:rubentxu" },
                        "properties": { "name": "Payments Service" }
                    }] }
                }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(draft["base_revision"], 6);
    let intent_id = draft["intent"]
        .as_str()
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .to_owned();

    // Preview is fresh.
    let (status, detail) = call(
        app.clone(),
        Request::get(format!("/intents/{intent_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["summary"]["stale"], false);
    assert_eq!(detail["summary"]["status"], "draft");

    // Explicit promotion applies the governed patch.
    let (status, outcome) = call(
        app.clone(),
        Request::post(format!("/intents/{intent_id}/promote"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "actor": "user:rubentxu" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["outcome"], "applied");
    assert_eq!(outcome["revision"], 7);

    // The rename landed in the graph.
    let (status, subject) = call(
        app,
        Request::get("/subjects/arch/container/payment-service")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(subject["properties"]["name"], "Payments Service");
}

#[tokio::test]
async fn sddk_owned_intent_promotes_to_governance_route() {
    let store = vistalith_graph::GraphStore::from_fixture_path(
        "../vistalith-graph/tests/fixtures/sample-world.json",
    )
    .expect("fixture replays");
    let app = router(AppState::new(store));

    let (status, draft) = call(
        app.clone(),
        Request::post("/intents")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "target": "sddk:work-item:TEST-MODEL-001",
                    "gesture": "rename",
                    "change": { "operations": [{
                        "op": "upsert-subject",
                        "subject": { "namespace": "sddk", "kind": "work-item", "id": "TEST-MODEL-001" },
                        "authority": "authoritative",
                        "provenance": { "source": "user:api" },
                        "properties": { "title": "hijack" }
                    }] }
                }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let intent_id = draft["intent"]
        .as_str()
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .to_owned();

    let (status, outcome) = call(
        app,
        Request::post(format!("/intents/{intent_id}/promote"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["outcome"], "sddk-governed");
    assert_eq!(outcome["subject"], "sddk:work-item:TEST-MODEL-001");
}

#[tokio::test]
async fn stale_intent_promotion_returns_conflict() {
    let store = vistalith_graph::GraphStore::from_fixture_path(
        "../vistalith-graph/tests/fixtures/sample-world.json",
    )
    .expect("fixture replays");
    let app = router(AppState::new(store));

    let (_, draft) = call(
        app.clone(),
        Request::post("/intents")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "target": "arch:container:payment-service",
                    "gesture": "rename",
                    "change": { "operations": [] }
                }"#,
            ))
            .unwrap(),
    )
    .await;
    let intent_id = draft["intent"]
        .as_str()
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .to_owned();

    // The graph moves on: an unrelated advisory subject lands.
    let (status, _) = call(
        app.clone(),
        Request::post("/patches")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "patch_id": "p-unrelated",
                    "base_revision": 6,
                    "proposed_by": "user:other",
                    "operations": [{
                        "op": "upsert-subject",
                        "subject": { "namespace": "visual", "kind": "note", "id": "n9" },
                        "authority": "advisory",
                        "provenance": { "source": "user:other" }
                    }]
                }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Preview now reports stale...
    let (_, detail) = call(
        app.clone(),
        Request::get(format!("/intents/{intent_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(detail["summary"]["stale"], true);

    // ...and promotion is denied with 409.
    let (status, outcome) = call(
        app,
        Request::post(format!("/intents/{intent_id}/promote"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(outcome["outcome"], "stale");
}

// --- Fork / diff / time travel (slice 5, SPEC-011) ---------------------------

#[tokio::test]
async fn thread_fork_diff_and_time_travel_over_http() {
    let app = router(AppState::new(vistalith_graph::GraphStore::new()));

    // One source thread with one turn.
    let (_, body) = call(
        app.clone(),
        Request::post("/threads")
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "title": "to be forked" }"#))
            .unwrap(),
    )
    .await;
    let thread_id = body["thread"].as_str().unwrap().rsplit(':').next().unwrap().to_owned();
    let (status, _) = call(
        app.clone(),
        Request::post(format!("/threads/{thread_id}/messages"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "content": "turn one" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Revision before the fork; then fork at the latest turn.
    let (_, graph) = call(
        app.clone(),
        Request::get("/graph").body(Body::empty()).unwrap(),
    )
    .await;
    let before_fork = graph["revision"].as_u64().unwrap();

    let (status, fork) = call(
        app.clone(),
        Request::post(format!("/threads/{thread_id}/fork"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "note": "explore cheaper model" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(fork["up_to_turn"], 1);
    assert!(fork["copied_events"].as_u64().unwrap() > 0);
    let fork_id = fork["fork"].as_str().unwrap().to_owned();

    // The fork appears as a first-class thread, linked back to its source.
    let (_, threads) = call(
        app.clone(),
        Request::get("/threads").body(Body::empty()).unwrap(),
    )
    .await;
    let forked = threads["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["thread"] == serde_json::json!(fork_id))
        .expect("fork is listed");
    assert_eq!(
        forked["forked_from"],
        serde_json::json!(format!("agentic:thread:{thread_id}"))
    );
    assert!(forked["title"]
        .as_str()
        .unwrap()
        .contains("fork ≤ turn 1"));

    // Time travel: the graph at `before_fork` has no fork thread...
    let (_, past) = call(
        app.clone(),
        Request::get(format!("/graph?at_revision={before_fork}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(past["as_of_revision"], serde_json::json!(before_fork));
    let past_has_fork = past["subjects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["subject"]["id"] == fork_id.rsplit(':').next().unwrap());
    assert!(!past_has_fork, "the fork must not exist in the past");

    // ...and the structural diff between the two revisions adds exactly it.
    let (_, diff) = call(
        app.clone(),
        Request::get(format!("/diff?from={before_fork}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    // SubjectRefs serialize in the flat wire format ({namespace, kind, id}).
    let fork_uuid = fork_id.rsplit(':').next().unwrap();
    let added: Vec<&str> = diff["added_subjects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert!(added.contains(&fork_uuid));

    // Unknown revisions are 4xx, not 500.
    let (status, _) = call(
        app,
        Request::get("/graph?at_revision=99999")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
