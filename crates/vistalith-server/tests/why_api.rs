//! SDDK workflow projection (M6) and why-path (M9) over HTTP.

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

/// A fixture store holding the SDDK observation chain:
/// code implements arch, decision derived, evidence provides_evidence_for.
fn fixture_app() -> Router {
    let store = vistalith_graph::GraphStore::from_fixture_path(
        "../vistalith-graph/tests/fixtures/sample-world.json",
    )
    .expect("fixture loads");
    router(AppState::with_runtime(
        store,
        RuntimeProvider::Fake(FakeProvider::repeating("ok")),
    ))
}

#[tokio::test]
async fn sddk_endpoints_require_the_bridge() {
    let app = fixture_app();
    let (status, _) = call(
        app.clone(),
        Request::post("/sddk/sync").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        app,
        Request::get("/sddk/receipts").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn why_path_walks_the_evidence_chain() {
    let app = fixture_app();
    // The fixture: code:repository:payments-api implements
    // arch:container:payment-service; sddk:work-item:TEST-MODEL-001
    // provides evidence via an advisory `affects` edge (advisory class).
    let (status, why) = call(
        app.clone(),
        Request::get("/why/arch/container/payment-service?depth=2")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(why["subject"], "arch:container:payment-service");
    // The implements edge from the code repository shows up as support.
    let links = why["links"].as_array().unwrap();
    assert!(
        links.iter().any(|l| {
            l["kind"] == serde_json::json!("implements")
                && l["from"] == serde_json::json!("code:repository:payments-api")
        }),
        "code support surfaces in the why path: {why}"
    );
    // Unknown subjects 404.
    let (status, _) = call(
        app,
        Request::get("/why/arch/container/ghost")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
