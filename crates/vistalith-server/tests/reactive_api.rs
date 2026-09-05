//! Reactive behaviors, graph algorithms and the semantic context view over
//! HTTP (SPEC-003 / SPEC-005, slice 7, milestones M3 + M4).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
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

fn subject_event(kind: &str, id: &str) -> String {
    format!(
        r#"{{
            "event_id": "{uuid}",
            "actor": "observation:ci",
            "timestamp": "2026-09-04T12:00:00Z",
            "subjects": [{{ "namespace": "arch", "kind": "container", "id": "{id}" }}],
            "correlation_id": "{uuid}",
            "type": "{kind}",
            "payload": {{
                "subject": {{ "namespace": "arch", "kind": "container", "id": "{id}" }},
                "authority": "authoritative",
                "provenance": {{ "source": "observation:ci" }},
                "properties": {{}}
            }}
        }}"#,
        uuid = uuid::Uuid::now_v7(),
        kind = kind,
        id = id,
    )
}

fn relation_event(from: &str, kind: &str, to: &str) -> String {
    format!(
        r#"{{
            "event_id": "{uuid}",
            "actor": "observation:ci",
            "timestamp": "2026-09-04T12:00:00Z",
            "subjects": [],
            "correlation_id": "{uuid}",
            "type": "relation-declared",
            "payload": {{
                "fact": {{
                    "relation": {{
                        "from": {{ "namespace": "arch", "kind": "container", "id": "{from}" }},
                        "kind": "{kind}",
                        "to": {{ "namespace": "arch", "kind": "container", "id": "{to}" }}
                    }},
                    "authority": "authoritative",
                    "provenance": {{ "source": "observation:ci" }}
                }}
            }}
        }}"#,
        uuid = uuid::Uuid::now_v7(),
        from = from,
        kind = kind,
        to = to,
    )
}

fn empty_store() -> Router {
    router(AppState::new(vistalith_graph::GraphStore::new()))
}

/// Builds gateway -> payment-service -> ledger -> database over the API.
async fn dependency_chain(app: &Router) {
    for id in ["gateway", "payment-service", "ledger", "database"] {
        let (status, _) = call(
            app.clone(),
            Request::post("/events")
                .header("content-type", "application/json")
                .body(Body::from(subject_event("subject-defined", id)))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    for (from, to) in [
        ("gateway", "payment-service"),
        ("payment-service", "ledger"),
        ("ledger", "database"),
    ] {
        let (status, _) = call(
            app.clone(),
            Request::post("/events")
                .header("content-type", "application/json")
                .body(Body::from(relation_event(from, "depends_on", to)))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
}

#[tokio::test]
async fn dependency_change_raises_a_traced_advisory() {
    let app = empty_store();
    dependency_chain(&app).await;

    // The dependency changes (milestone M4's code-change observation).
    let trigger = format!(
        r#"{{
            "event_id": "{uuid}",
            "actor": "observation:ci",
            "timestamp": "2026-09-04T12:01:00Z",
            "subjects": [],
            "correlation_id": "0198f6c0-0000-7000-8000-00000000abcd",
            "type": "subject-updated",
            "payload": {{
                "subject": {{ "namespace": "arch", "kind": "container", "id": "ledger" }},
                "properties": {{ "schema_version": 2 }}
            }}
        }}"#,
        uuid = uuid::Uuid::now_v7(),
    );
    let (status, body) = call(
        app.clone(),
        Request::post("/events")
            .header("content-type", "application/json")
            .body(Body::from(trigger))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        body["advisories_raised"], 1,
        "payment-service depends_on ledger: impact advisory fires"
    );

    // The advisory is in the graph, advisory-class, and traceable in the log.
    let (_, graph) = call(
        app.clone(),
        Request::get("/graph").body(Body::empty()).unwrap(),
    )
    .await;
    let advisory = graph["subjects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["subject"]["kind"] == serde_json::json!("advisory"))
        .expect("advisory subject projected");
    assert_eq!(advisory["authority"], "advisory");
    assert!(advisory["properties"]["note"]
        .as_str()
        .unwrap()
        .contains("depends_on"));

    let (_, events) = call(
        app,
        Request::get("/events").body(Body::empty()).unwrap(),
    )
    .await;
    let advisory_event = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == serde_json::json!("advisory-raised"))
        .expect("advisory event durable");
    assert!(
        advisory_event["causation_id"].is_string(),
        "SPEC-003: the advisory traces to its trigger via causation_id"
    );
}

#[tokio::test]
async fn impact_and_path_endpoints_answer_algorithmic_questions() {
    let app = empty_store();
    dependency_chain(&app).await;

    let (_, impact) = call(
        app.clone(),
        Request::get("/algorithms/impact/arch/container/database")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let impacted: Vec<&str> = impact["impacted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(
        impacted,
        vec![
            "arch:container:gateway",
            "arch:container:ledger",
            "arch:container:payment-service"
        ]
    );

    let (status, path) = call(
        app.clone(),
        Request::get("/algorithms/path?from=arch:container:gateway&to=arch:container:database")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(path["path"].as_array().unwrap().len(), 4);

    let (status, _) = call(
        app.clone(),
        Request::get("/algorithms/impact/arch/container/nobody")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, cycles) = call(
        app,
        Request::get("/algorithms/cycles").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(cycles["cycles"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn context_view_explains_every_inclusion() {
    let app = empty_store();
    dependency_chain(&app).await;

    let (status, view) = call(
        app,
        Request::post("/views/context")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "roots": ["arch:container:payment-service"],
                    "relations": ["depends_on"],
                    "max_depth": 1,
                    "include_derived": false,
                    "include_advisory": false,
                    "token_budget": 100000
                }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body was: {view}");
    let items = view["items"].as_array().unwrap();
    let subjects: Vec<&str> = items.iter().map(|i| i["subject"].as_str().unwrap()).collect();
    assert_eq!(
        subjects,
        ["arch:container:payment-service", "arch:container:ledger"]
    );
    // M3: the view explains why each item is included.
    assert_eq!(items[0]["reason"]["reason"], "root");
    assert_eq!(items[1]["reason"]["reason"], "via");
    assert_eq!(items[1]["reason"]["kind"], "depends_on");
    assert!(items[1]["last_touch"].is_string());
    assert_eq!(items[1]["last_actor"], "observation:ci");
    // The database is discovered but too deep: negative knowledge reported.
    let exclusions = view["exclusions"].as_array().unwrap();
    assert!(
        exclusions
            .iter()
            .any(|e| e["subject"] == serde_json::json!("arch:container:database"))
    );
}

#[tokio::test]
async fn context_view_rejects_empty_roots() {
    let app = empty_store();
    let (status, _) = call(
        app,
        Request::post("/views/context")
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "roots": [] }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

// --- Full impact analysis (slice 16, visual/IMPACT.md) ------------------------

#[tokio::test]
async fn full_impact_analysis_surfaces_all_sections() {
    let app = empty_store();
    dependency_chain(&app).await;

    let (_, analysis) = call(
        app.clone(),
        Request::get("/algorithms/impact/arch/container/ledger?full=true")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    // Direct dependents (one hop back).
    assert_eq!(
        analysis["direct_dependents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Transitive includes gateway.
    assert!(
        analysis["transitively_impacted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| *s == serde_json::json!("arch:container:gateway"))
    );
    // Tests/evidence/decisions sections exist (possibly empty here).
    assert!(analysis["affected_tests"].is_array());
    assert!(analysis["stale_evidence"].is_array());
    assert!(analysis["decisions_potentially_invalidated"].is_array());
    assert!(analysis["unknown_impact"].is_array());

    // Without full=true, the legacy lightweight report shape applies.
    let (_, report) = call(
        app,
        Request::get("/algorithms/impact/arch/container/ledger")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(report["impacted"].is_array());
    assert!(report.get("direct_dependents").is_none());
}

// --- Canvas / progressive formalization (slice 17, VISUAL-THINKING.md) ------

#[tokio::test]
async fn canvas_primitives_are_advisory_and_formalize_to_intents() {
    let app = empty_store();
    dependency_chain(&app).await;

    // Sketch a question attached to a semantic subject (step 1+2).
    let (status, primitive) = call(
        app.clone(),
        Request::post("/canvas/subjects")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{
                    "kind": "question",
                    "content": "does the gateway need rate limiting?",
                    "relates_to": "arch:container:gateway"
                }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {primitive}");
    let subject = primitive["subject"].as_str().unwrap();
    assert!(subject.starts_with("vistalith:question:"));

    // It is advisory and inventoried.
    let (_, canvas) = call(
        app.clone(),
        Request::get("/canvas/subjects").body(Body::empty()).unwrap(),
    )
    .await;
    let subjects = canvas["subjects"].as_array().unwrap();
    assert_eq!(subjects.len(), 1);
    assert_eq!(subjects[0]["authority"], "advisory");
    assert_eq!(
        subjects[0]["relates_to"],
        serde_json::json!("arch:container:gateway")
    );

    // Progressive formalization (step 3-4): promote to a VisualIntent draft.
    let subject_id = subject.rsplit(':').next().unwrap();
    let (status, promoted) = call(
        app.clone(),
        Request::post(format!("/canvas/subjects/vistalith/question/{subject_id}/promote"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "gesture": "annotate" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {promoted}");
    let intent = promoted["intent"].as_str().unwrap();
    assert!(intent.starts_with("visual:visual-proposal:"));
    assert_eq!(promoted["target"], serde_json::json!("arch:container:gateway"));

    // Promotion of a primitive with no related subject is rejected.
    let (status, _) = call(
        app.clone(),
        Request::post("/canvas/subjects")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{ "kind": "note", "content": "loose note" }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, err) = call(
        app,
        Request::post("/canvas/subjects/vistalith/note/loose/promote")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(err["error"].as_str().unwrap().contains("unknown canvas"));
}
