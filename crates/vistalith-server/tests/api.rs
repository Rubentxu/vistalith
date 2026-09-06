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

// --- LikeC4 round-trip (slice 19, SPK-008) ------------------------------------

async fn call_text(app: Router, request: Request<Body>) -> (StatusCode, String, String) {
    let response = app.oneshot(request).await.expect("in-memory service");
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .expect("content type")
        .to_str()
        .expect("ascii header")
        .to_owned();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, content_type, String::from_utf8(bytes.to_vec()).expect("utf-8"))
}

#[tokio::test]
async fn likec4_export_embeds_identity_and_reimport_is_a_noop() {
    let app = app();
    let (status, _, _) = call_text(
        app.clone(),
        Request::post("/events")
            .header("content-type", "application/json")
            .body(Body::from(SAMPLE_EVENT))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // export: the SubjectRef travels inside the DSL metadata
    let (status, content_type, dsl) = call_text(
        app.clone(),
        Request::get("/views/c4/likec4").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/plain"));
    assert!(dsl.contains("vistalith 'arch:system:vistalith'"));
    assert!(dsl.contains("system vistalith \"Vistalith\""));

    // re-import of the untouched export changes nothing
    let (status, json) = call(
        app.clone(),
        Request::post("/views/c4/likec4")
            .header("content-type", "text/plain")
            .body(Body::from(dsl.clone()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["defined_subjects"].as_array().unwrap().len(), 0);
    assert_eq!(json["updated_subjects"].as_array().unwrap().len(), 0);
    assert_eq!(json["unchanged_subjects"].as_array().unwrap().len(), 1);
    assert_eq!(json["deprecated_subjects"].as_array().unwrap().len(), 0);
    assert_eq!(json["declared_relations"].as_array().unwrap().len(), 0);

    // and the graph revision did not move
    let (_, json) = call(app, Request::get("/graph").body(Body::empty()).unwrap()).await;
    assert_eq!(json["revision"], 1);
}

#[tokio::test]
async fn likec4_import_of_foreign_model_creates_fqn_subjects() {
    let dsl = r#"
        model {
            system billing {
                container worker "Worker"
            }
            billing -[depends_on]-> billing.worker
        }
    "#;
    let (status, json) = call(
        app(),
        Request::post("/views/c4/likec4")
            .header("content-type", "text/plain")
            .body(Body::from(dsl))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["defined_subjects"].as_array().unwrap().len(), 2);
    let ids: Vec<String> = json["defined_subjects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            format!(
                "{}:{}:{}",
                s["namespace"].as_str().unwrap_or_default(),
                s["kind"].as_str().unwrap_or_default(),
                s["id"].as_str().unwrap_or_default()
            )
        })
        .collect();
    assert!(ids.contains(&"arch:system:billing".to_owned()));
    assert!(ids.contains(&"arch:container:billing.worker".to_owned()));
    assert_eq!(json["declared_relations"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn likec4_import_rejects_broken_dsl() {
    let (status, json) = call(
        app(),
        Request::post("/views/c4/likec4")
            .header("content-type", "text/plain")
            .body(Body::from("model { system a a -> ghost }"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(json["error"]
        .as_str()
        .expect("error message")
        .contains("unknown element"));
}

#[tokio::test]
async fn c4_diff_endpoint_reports_architecture_changes() {
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

    let (_, json) = call(
        app,
        Request::get("/views/c4/diff?from=0").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(json["from_revision"], 0);
    assert_eq!(json["to_revision"], 1);
    let added = json["added_elements"].as_array().unwrap();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0]["identity"], "arch:system:vistalith");
    assert!(json["removed_elements"].as_array().unwrap().is_empty());
    assert!(json["changed_elements"].as_array().unwrap().is_empty());
}

// --- Excalidraw bindings (slice 20, SPK-009) ----------------------------------


#[tokio::test]
async fn excalidraw_import_binds_and_round_trips_as_noop() {
    let app = app();

    // a canvas note primitive exists (slice-17 flow)
    let (status, note) = call(
        app.clone(),
        Request::post("/canvas/subjects")
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "kind": "note", "content": "Remember the milk" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let identity = note["subject"].as_str().expect("subject ref");

    // import a scene binding a shape to the note via customData
    let scene = serde_json::json!({
        "type": "excalidraw",
        "elements": [{
            "id": "shape-1",
            "type": "text",
            "text": "Remember the milk",
            "x": 10.0, "y": 20.0, "width": 120.0, "height": 40.0,
            "customData": { "vistalith": identity },
        }],
    });
    let (status, report) = call(
        app.clone(),
        Request::post("/canvas/excalidraw")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&scene).unwrap()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["bound"].as_array().unwrap().len(), 1);

    // bindings read model
    let (_, bindings) = call(
        app.clone(),
        Request::get("/canvas/bindings").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(bindings["bindings"].as_array().unwrap().len(), 1);
    assert_eq!(bindings["bindings"][0]["via"], "custom-data");
    assert_eq!(bindings["bindings"][0]["shape_id"], "shape-1");

    // export: the scene carries the identity in customData
    let (_, exported) = call(
        app.clone(),
        Request::get("/canvas/excalidraw").body(Body::empty()).unwrap(),
    )
    .await;
    let elements = exported["elements"].as_array().unwrap();
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0]["customData"]["vistalith"], identity);

    // re-import the export: shape id differs (element id = subject id),
    // content identical → no-op, revision unchanged
    let (_, health) = call(app.clone(), Request::get("/health").body(Body::empty()).unwrap()).await;
    let before = health["graph_revision"].as_u64().unwrap();
    let (status, report) = call(
        app.clone(),
        Request::post("/canvas/excalidraw")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&exported).unwrap()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["bound"].as_array().unwrap().len(), 0);
    assert_eq!(report["skipped_bindings"].as_array().unwrap().len(), 1);
    let (_, health) = call(app, Request::get("/health").body(Body::empty()).unwrap()).await;
    assert_eq!(health["graph_revision"].as_u64().unwrap(), before);
}

#[tokio::test]
async fn excalidraw_create_missing_makes_primitives() {
    let app = app();
    let scene = serde_json::json!({
        "type": "excalidraw",
        "elements": [{
            "id": "shape-9",
            "type": "text",
            "text": "A fresh idea from the whiteboard",
        }],
    });
    let (status, report) = call(
        app.clone(),
        Request::post("/canvas/excalidraw?create_missing=true")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&scene).unwrap()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["created_primitives"].as_array().unwrap().len(), 1);

    // the primitive shows up in the canvas subjects lens
    let (_, canvas) = call(
        app,
        Request::get("/canvas/subjects").body(Body::empty()).unwrap(),
    )
    .await;
    let subjects = canvas["subjects"].as_array().unwrap();
    assert!(subjects
        .iter()
        .any(|s| s["content"] == "A fresh idea from the whiteboard"));
}

#[tokio::test]
async fn excalidraw_broken_scene_is_rejected() {
    let (status, json) = call(
        app(),
        Request::post("/canvas/excalidraw")
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "type": "excalidraw" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(json["error"].as_str().expect("error").contains("elements"));
}
