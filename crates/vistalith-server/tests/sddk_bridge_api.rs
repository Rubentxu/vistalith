//! Governed SDDK promotion over HTTP (SPK-012 / milestone M7): with the
//! bridge configured, promoting an intent on an SDDK-owned subject submits
//! the proposal through SDDK's capability gateway; the decision and receipt
//! are durable, and denied/approval flows never execute anything.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;
use vistalith_agent_runtime::{FakeProvider, RuntimeProvider};
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

fn fixture_store() -> vistalith_graph::GraphStore {
    vistalith_graph::GraphStore::from_fixture_path(
        "../vistalith-graph/tests/fixtures/sample-world.json",
    )
    .expect("fixture loads")
}

fn app_with_bridge(workflow: &str) -> Router {
    let ledger = std::env::temp_dir().join(format!(
        "vistalith-server-bridge-{}.sqlite",
        uuid::Uuid::now_v7()
    ));
    let workflow_path = std::env::temp_dir().join(format!(
        "vistalith-server-workflow-{}.json",
        uuid::Uuid::now_v7()
    ));
    std::fs::write(&workflow_path, workflow).unwrap();
    let bridge = vistalith_sddk_bridge::SddkBridge::open(ledger, workflow_path, "TEST-PROJECT")
        .expect("bridge opens");
    router(
        AppState::with_runtime(
            fixture_store(),
            RuntimeProvider::Fake(FakeProvider::repeating("ok")),
        )
        .with_sddk_bridge(bridge),
    )
}

/// Drafts an intent on the fixture's observed SDDK work item.
async fn draft_sddk_intent(app: &Router) -> String {
    let (status, body) = call(
        app.clone(),
        Request::post("/intents")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "target": "sddk:work-item:TEST-MODEL-001",
                    "gesture": "annotate",
                    "reason": "impact assessment from the visual workspace",
                    "change": { "operations": [{
                        "op": "upsert-subject",
                        "subject": { "namespace": "sddk", "kind": "work-item", "id": "TEST-MODEL-001" },
                        "authority": "authoritative",
                        "provenance": { "source": "user:ruben" },
                        "properties": { "impact": "assessed" }
                    }] }
                }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "draft body: {body}");
    body["intent"]
        .as_str()
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn promotion_routes_through_the_sddk_gateway() {
    // Low-risk workflow: the gateway allows without approval.
    let app = app_with_bridge(
        r#"{
            "schema_version": 1,
            "workflow": { "id": "vistalith-bridge", "version": "0.1.0", "description": "allow fixture" },
            "statuses": ["OPEN", "CLOSED"],
            "phases": ["explore"],
            "policies": {},
            "transitions": [{ "id": "open", "to": { "status": "OPEN" }, "requires": [] }],
            "forge": { "provider": "vistalith", "capabilities": {
                "evidence.write": { "risk": "low", "consequence": "creates" }
            } }
        }"#,
    );
    let intent = draft_sddk_intent(&app).await;

    let (status, outcome) = call(
        app.clone(),
        Request::post(format!("/intents/{intent}/promote"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {outcome}");
    assert_eq!(outcome["outcome"], "submitted-to-sddk");
    assert_eq!(outcome["decision"], "allowed");
    assert!(outcome["proposal"].as_str().unwrap().starts_with("vistalith:proposal:"));
    assert!(outcome["receipt_id"].is_string());

    // The SDDK ledger holds the receipt (B1: the receipt comes from SDDK's
    // own storage, not a Vistalith copy).
    let (_, receipts) = call(
        app.clone(),
        Request::get("/sddk/receipts").body(Body::empty()).unwrap(),
    )
    .await;
    let receipts = receipts["receipts"].as_array().unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0]["capability"], serde_json::json!("evidence.write"));
    assert_eq!(receipts[0]["status"], serde_json::json!("succeeded"));

    // The intent resolves as submitted; the graph holds the proposal as a
    // derived observation providing evidence for the SDDK subject.
    let (_, graph) = call(
        app,
        Request::get("/graph").body(Body::empty()).unwrap(),
    )
    .await;
    let proposal = graph["subjects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["subject"]["kind"] == serde_json::json!("proposal"))
        .expect("proposal subject projected");
    assert_eq!(proposal["authority"], "derived");
    assert_eq!(proposal["properties"]["decision"], serde_json::json!("allowed"));
    assert!(graph["relations"].as_array().unwrap().iter().any(|r| {
        r["relation"]["kind"] == serde_json::json!("provides_evidence_for")
            && r["relation"]["from"]["kind"] == serde_json::json!("proposal")
            && r["relation"]["to"]["id"] == serde_json::json!("TEST-MODEL-001")
    }));
}

#[tokio::test]
async fn approval_required_gate_stops_execution_until_approved() {
    // High-risk workflow: approval required, and the endpoint's approve flag
    // is the human decision.
    let app = app_with_bridge(
        r#"{
            "schema_version": 1,
            "workflow": { "id": "vistalith-bridge", "version": "0.1.0", "description": "approval fixture" },
            "statuses": ["OPEN", "CLOSED"],
            "phases": ["explore"],
            "policies": {},
            "transitions": [{ "id": "open", "to": { "status": "OPEN" }, "requires": [] }],
            "forge": { "provider": "vistalith", "capabilities": {
                "evidence.write": { "risk": "high", "consequence": "creates" }
            } }
        }"#,
    );
    let intent = draft_sddk_intent(&app).await;

    // Without approval: recorded, nothing executed.
    let (status, outcome) = call(
        app.clone(),
        Request::post(format!("/intents/{intent}/promote"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["decision"], "approval-required");
    assert_eq!(outcome["receipt_id"], serde_json::Value::Null);

    // No receipts in the ledger: the capability never ran.
    let (_, receipts) = call(
        app,
        Request::get("/sddk/receipts").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(receipts["receipts"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn without_a_bridge_the_legacy_governance_route_applies() {
    let app = router(AppState::with_runtime(
        fixture_store(),
        RuntimeProvider::Fake(FakeProvider::repeating("ok")),
    ));
    let intent = draft_sddk_intent(&app).await;
    let (status, outcome) = call(
        app,
        Request::post(format!("/intents/{intent}/promote"))
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outcome["outcome"], "sddk-governed");
}
