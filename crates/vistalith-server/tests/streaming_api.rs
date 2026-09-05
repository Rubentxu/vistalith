//! SSE streaming endpoint (slice 11): deltas flow while the turn runs, and
//! the terminal `done` frame carries the same durable coordinates as the
//! non-streamed endpoint.

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
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("JSON body")
    };
    (status, json)
}

fn app() -> Router {
    router(AppState::with_runtime(
        vistalith_graph::GraphStore::new(),
        RuntimeProvider::Fake(FakeProvider::steps(vec![
            FakeStep::Text("alpha beta gamma delta epsilon".to_owned()),
            FakeStep::Text("second turn".to_owned()),
        ])),
    ))
}

#[tokio::test]
async fn streaming_turn_emits_sse_frames_and_is_durable() {
    let app = app();

    let (_, body) = call(
        app.clone(),
        Request::post("/threads")
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "title": "stream" }"#))
            .unwrap(),
    )
    .await;
    let thread = body["thread"].as_str().unwrap().rsplit(':').next().unwrap().to_owned();

    // POST to the SSE endpoint; collect the whole body as text.
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/threads/{thread}/messages/stream"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{ "content": "stream this" }"#))
                .unwrap(),
        )
        .await
        .expect("in-memory service");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();

    // Frames: 3+ deltas then exactly one done frame.
    let delta_texts: Vec<String> = text
        .split("\n\n")
        .filter(|frame| frame.starts_with("event: delta"))
        .map(|frame| {
            frame
                .strip_prefix("event: delta\ndata: ")
                .unwrap()
                .to_owned()
        })
        .collect();
    assert!(
        delta_texts.len() >= 2,
        "expected multiple deltas, got {text:?}"
    );

    let done_frames: Vec<&str> = text
        .split("\n\n")
        .filter(|frame| frame.starts_with("event: done"))
        .collect();
    assert_eq!(done_frames.len(), 1, "exactly one done frame: {text:?}");
    let done: serde_json::Value =
        serde_json::from_str(done_frames[0].strip_prefix("event: done\ndata: ").unwrap())
            .expect("done frame is JSON");
    assert_eq!(done["turn"], 1);
    assert_eq!(done["content"], "alpha beta gamma delta epsilon");
    assert_eq!(
        delta_texts.concat(),
        done["content"].as_str().unwrap(),
        "deltas concatenate to the terminal content"
    );
    assert!(done["usage"]["total_tokens"].as_u64().unwrap() > 0);

    // Durability: the streamed turn is in the log like any other.
    let (_, graph) = call(
        app,
        Request::get("/graph").body(Body::empty()).unwrap(),
    )
    .await;
    let thread_node = graph["subjects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["subject"]["id"] == serde_json::json!(thread))
        .unwrap();
    assert_eq!(thread_node["properties"]["turns"], serde_json::json!(1));
}

#[tokio::test]
async fn streaming_unknown_thread_404s_before_the_stream_opens() {
    let (status, _) = call(
        app(),
        Request::post("/threads/ghost/messages/stream")
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "content": "x" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
