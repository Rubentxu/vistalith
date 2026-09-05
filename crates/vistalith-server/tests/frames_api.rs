//! Agents & frames over HTTP (slice 8): durable bounded execution — a frame
//! owns a thread, its turns are enforced against budgets, and closed frames
//! refuse further turns.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;
use vistalith_agent_runtime::{FakeProvider, FakeStep, RuntimeProvider};
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

fn fixture_app() -> Router {
    let store = vistalith_graph::GraphStore::from_fixture_path(
        "../vistalith-graph/tests/fixtures/sample-world.json",
    )
    .expect("fixture loads");
    router(AppState::with_runtime(
        store,
        RuntimeProvider::Fake(FakeProvider::steps(vec![
            FakeStep::Text("frame answer one".to_owned()),
            FakeStep::Text("frame answer two".to_owned()),
        ])),
    ))
}

#[tokio::test]
async fn agent_registration_is_durable() {
    let app = fixture_app();
    let (status, body) = call(
        app.clone(),
        Request::post("/agents")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "role": "impact analyst",
                    "instructions": "Assess dependency impact before answering.",
                    "model": "anthropic/claude-haiku-4-5",
                    "tools": ["graph_search"],
                    "budget_turns": 4,
                    "expected_outputs": ["findings", "risks"]
                }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let agent = body["agent"].as_str().unwrap().to_owned();
    assert!(agent.starts_with("agentic:agent:"));

    let (_, agents) = call(
        app,
        Request::get("/agents").body(Body::empty()).unwrap(),
    )
    .await;
    let listed = agents["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["agent"] == serde_json::json!(agent))
        .expect("agent listed");
    assert_eq!(listed["role"], "impact analyst");
    assert_eq!(listed["budget_turns"], 4);
}

#[tokio::test]
async fn frame_lifecycle_over_http() {
    let app = fixture_app();

    // Start a frame bounded to the fixture subject with one permitted tool.
    let (status, body) = call(
        app.clone(),
        Request::post("/frames")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "goal": "assess payment service impact",
                    "subjects": ["arch:container:payment-service"],
                    "permitted_tools": ["graph_search"],
                    "max_turns": 2,
                    "token_budget": 100000
                }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let frame = body["frame"].as_str().unwrap().to_owned();
    assert!(frame.starts_with("agentic:frame:"));
    assert!(body["thread"].as_str().unwrap().starts_with("agentic:thread:"));
    let frame_id = frame.rsplit(':').next().unwrap().to_owned();

    // The frame lists as open with its bounds.
    let (_, frames) = call(
        app.clone(),
        Request::get("/frames").body(Body::empty()).unwrap(),
    )
    .await;
    let listed = frames["frames"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["frame"] == serde_json::json!(frame))
        .expect("frame listed");
    assert_eq!(listed["status"], "open");
    assert_eq!(listed["max_turns"], 2);
    assert_eq!(
        listed["permitted_tools"],
        serde_json::json!(["graph_search"])
    );

    // A bounded turn runs against the scripted provider.
    let (status, turn) = call(
        app.clone(),
        Request::post(format!("/frames/{frame_id}/turns"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "content": "assess away" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(turn["turn"], 1);
    assert_eq!(turn["auto_closed"], serde_json::Value::Null);
    assert_eq!(turn["content"]["turns_used"], 1);

    // The frame view includes the durable thread messages.
    let (_, view) = call(
        app.clone(),
        Request::get(format!("/frames/{frame_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let messages = view["messages"].as_array().unwrap();
    assert!(messages
        .iter()
        .any(|m| m["content"] == serde_json::json!("frame answer one")));

    // Explicit close wins; the summary is durable.
    let (status, closed) = call(
        app.clone(),
        Request::post(format!("/frames/{frame_id}/close"))
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{ "outcome": "completed", "summary": "impact assessed" }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(closed["status"], "completed");
    assert_eq!(closed["summary"], "impact assessed");

    // A closed frame refuses turns with 409.
    let (status, err) = call(
        app,
        Request::post(format!("/frames/{frame_id}/turns"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "content": "late" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(err["error"].as_str().unwrap().contains("closed"));
}

#[tokio::test]
async fn turn_budget_auto_closes_the_frame() {
    let store = vistalith_graph::GraphStore::from_fixture_path(
        "../vistalith-graph/tests/fixtures/sample-world.json",
    )
    .unwrap();
    let app = router(AppState::with_runtime(
        store,
        RuntimeProvider::Fake(FakeProvider::repeating("ok")),
    ));

    let (status, body) = call(
        app.clone(),
        Request::post("/frames")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "goal": "single turn",
                    "subjects": ["arch:container:payment-service"],
                    "max_turns": 1,
                    "token_budget": 100000
                }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let frame_id = body["frame"].as_str().unwrap().rsplit(':').next().unwrap().to_owned();

    let (status, turn) = call(
        app.clone(),
        Request::post(format!("/frames/{frame_id}/turns"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "content": "the only turn" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(turn["auto_closed"], "turns-exhausted");

    let (_, view) = call(
        app,
        Request::get(format!("/frames/{frame_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(view["frame"]["status"], "turns-exhausted");
}

#[tokio::test]
async fn unknown_frame_subjects_are_rejected() {
    let app = fixture_app();
    let (status, err) = call(
        app,
        Request::post("/frames")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{ "goal": "bad", "subjects": ["arch:container:ghost"] }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(err["error"].as_str().unwrap().contains("ghost"));
}

// --- UAT checks + lens (slice 15, UAT-STUDIO.md) ------------------------------

#[tokio::test]
async fn uat_scenario_checks_are_durable_and_inventoried() {
    let app = fixture_app();

    // Define a scenario subject (derived: it would be projected from SDDK).
    let scenario_json = r#"{
        "event_id": "0198f6c0-0000-7000-8000-00000000u001",
        "actor": "observation:sddk",
        "timestamp": "2026-09-05T12:00:00Z",
        "subjects": [],
        "correlation_id": "0198f6c0-0000-7000-8000-00000000u001",
        "type": "subject-defined",
        "payload": {
            "subject": { "namespace": "verification", "kind": "uat-scenario", "id": "uat-1" },
            "authority": "derived",
            "provenance": { "source": "observation:sddk" },
            "properties": { "title": "payment flow signoff" }
        }
    }"#;
    // event ids must be valid uuids
    let scenario_json = scenario_json.replace("0198f6c0-0000-7000-8000-00000000u001", "0198f6c0-7700-7000-8000-000000000001");
    let (status, _) = call(
        app.clone(),
        Request::post("/events")
            .header("content-type", "application/json")
            .body(Body::from(scenario_json))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Record a failing check, then a passing one.
    for (verdict, note) in [("fail", "payment declined"), ("pass", "retested OK")] {
        let (status, body) = call(
            app.clone(),
            Request::post("/uat/checks")
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{ "scenario": "verification:uat-scenario:uat-1", "verdict": "{verdict}", "note": "{note}" }}"#
            )))
            .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "body: {body}");
        assert_eq!(body["verdict"], serde_json::json!(verdict));
    }

    // The lens inventories the scenario with its checks and latest verdict.
    let (_, lens) = call(
        app,
        Request::get("/lens/uat").body(Body::empty()).unwrap(),
    )
    .await;
    let scenarios = lens["scenarios"].as_array().unwrap();
    assert_eq!(scenarios.len(), 1);
    let scenario = &scenarios[0];
    assert_eq!(scenario["title"], serde_json::json!("payment flow signoff"));
    assert_eq!(scenario["latest_verdict"], "pass");
    let checks = scenario["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[0]["verdict"], "fail");
    assert_eq!(checks[1]["verdict"], "pass");
}
