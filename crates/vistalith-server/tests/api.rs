use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use vistalith_server::{AppState, router};

fn app() -> Router {
    router(AppState::empty())
}

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

const SAMPLE_EVENT: &str = r#"{
  "event_id": "0198f6c0-0000-7000-8000-00000000000a",
  "actor": "test:api",
  "timestamp": "2026-09-04T10:00:00Z",
  "subjects": [
    { "namespace": "arch", "kind": "system", "id": "vistalith" }
  ],
  "correlation_id": "0198f6c0-0000-7000-8000-0000000000ff",
  "type": "subject-defined",
  "payload": {
    "subject": { "namespace": "arch", "kind": "system", "id": "vistalith" },
    "authority": "authoritative",
    "provenance": { "source": "test:api" },
    "properties": { "name": "Vistalith" }
  }
}"#;

const SAMPLE_PATCH: &str = r#"{
  "patch_id": "patch-api-001",
  "base_revision": 1,
  "proposed_by": "test:api",
  "operations": [
    {
      "op": "upsert-subject",
      "subject": { "namespace": "visual", "kind": "hypothesis", "id": "h-100" },
      "authority": "advisory",
      "provenance": { "source": "test:api" }
    }
  ]
}"#;

#[tokio::test]
async fn health_reports_graph_state() {
    let (status, body) = call(app(), Request::get("/health").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["graph_revision"], 0);
}

#[tokio::test]
async fn append_event_then_read_subject_back() {
    let app = app();

    let (status, body) = call(
        app.clone(),
        Request::post("/events")
            .header("content-type", "application/json")
            .body(Body::from(SAMPLE_EVENT))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["sequence"], 0);
    assert_eq!(body["revision"], 1);

    // Cross-lens rule: the SubjectRef is addressable, not a renderer id.
    let (status, subject) = call(
        app.clone(),
        Request::get("/subjects/arch/system/vistalith")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(subject["subject"]["id"], "vistalith");
    assert_eq!(subject["authority"], "authoritative");

    // Duplicate id -> 409, nothing appended.
    let (status, body) = call(
        app.clone(),
        Request::post("/events")
            .header("content-type", "application/json")
            .body(Body::from(SAMPLE_EVENT))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("duplicate"));

    // The log serves the stored event with its assigned coordinates.
    let (status, log) = call(app, Request::get("/events").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(log["events"].as_array().unwrap().len(), 1);
    assert_eq!(log["events"][0]["sequence"], 0);
    assert_eq!(log["events"][0]["type"], "subject-defined");
}

#[tokio::test]
async fn patch_outcomes_over_http() {
    let app = app();

    // Seed one event so the graph revision is 1.
    let (status, _) = call(
        app.clone(),
        Request::post("/events")
            .header("content-type", "application/json")
            .body(Body::from(SAMPLE_EVENT))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Patch against the current base: applied.
    let (status, body) = call(
        app.clone(),
        Request::post("/patches")
            .header("content-type", "application/json")
            .body(Body::from(SAMPLE_PATCH))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "applied");
    assert_eq!(body["revision"], 2);

    // Same base again: stale -> 409 with a rejected outcome.
    let (status, body) = call(
        app,
        Request::post("/patches")
            .header("content-type", "application/json")
            .body(Body::from(SAMPLE_PATCH))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["status"], "rejected");
    assert!(body["reason"].as_str().unwrap().contains("stale"));
}

#[tokio::test]
async fn graph_endpoint_is_canonical_and_revisioned() {
    let app = app();
    let (status, _) = call(
        app.clone(),
        Request::post("/events")
            .header("content-type", "application/json")
            .body(Body::from(SAMPLE_EVENT))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, graph) = call(app, Request::get("/graph").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(graph["revision"], 1);
    assert_eq!(graph["subjects"].as_array().unwrap().len(), 1);
    assert_eq!(graph["relations"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn browser_clients_can_preflight_the_api() {
    let response = app()
        .oneshot(
            Request::options("/subjects")
                .header("origin", "http://localhost:5173")
                .header("access-control-request-method", "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("in-memory service");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .expect("CORS header"),
        "*"
    );
}
