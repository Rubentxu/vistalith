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
