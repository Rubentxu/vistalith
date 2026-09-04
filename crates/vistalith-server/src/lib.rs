//! `vistalithd` — the tiny slice-1 server (IMPLEMENT-NOW.md item 7).
//!
//! In-memory event log + SWG projection behind a minimal HTTP API:
//! subjects/relations readable, events appendable, patches proposable.
//! Persistence, conversations, providers and lenses are later slices.

use std::sync::{Arc, RwLock};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use vistalith_domain::{Namespace, SubjectKind, SubjectRef, VEvent};
use vistalith_graph::{GraphPatch, GraphStore, PatchOutcome, StoreError, canonical_graph_json};

/// Shared state: the durable log and its projection behind one lock.
/// Handlers never await while holding it, so a std lock is enough.
#[derive(Clone)]
pub struct AppState {
    store: Arc<RwLock<GraphStore>>,
}

impl AppState {
    pub fn new(store: GraphStore) -> Self {
        AppState {
            store: Arc::new(RwLock::new(store)),
        }
    }

    pub fn empty() -> Self {
        AppState::new(GraphStore::new())
    }
}

pub fn router(state: AppState) -> Router {
    // Slice-2: the web client runs on a different origin in dev, so the API
    // is CORS-permissive for now. Tighten when the client is served by
    // vistalithd itself or embedded in Tauri.
    use axum::http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, ORIGIN};
    use tower_http::cors::{Any, CorsLayer};

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers([CONTENT_TYPE, ACCEPT, AUTHORIZATION, ORIGIN]);

    Router::new()
        .route("/health", get(health))
        .route("/graph", get(get_graph))
        .route("/subjects", get(get_subjects))
        .route("/subjects/{namespace}/{kind}/{id}", get(get_subject))
        .route("/events", get(get_events).post(post_event))
        .route("/patches", post(post_patch))
        .layer(cors)
        .with_state(state)
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: String) -> Self {
        ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(err: StoreError) -> Self {
        match &err {
            StoreError::DuplicateEventId(_) => ApiError {
                status: StatusCode::CONFLICT,
                message: err.to_string(),
            },
            _ => ApiError::bad_request(err.to_string()),
        }
    }
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().expect("store lock");
    Json(serde_json::json!({
        "status": "ok",
        "service": "vistalithd",
        "graph_revision": store.graph().revision(),
        "events": store.log().len(),
    }))
}

async fn get_graph(State(state): State<AppState>) -> Response {
    let store = state.store.read().expect("store lock");
    let body: serde_json::Value =
        serde_json::from_str(&canonical_graph_json(store.graph())).expect("canonical JSON");
    Json(body).into_response()
}

async fn get_subjects(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().expect("store lock");
    let subjects: Vec<_> = store.graph().subjects().collect();
    Json(serde_json::json!({ "subjects": subjects }))
}

async fn get_subject(
    State(state): State<AppState>,
    Path((namespace, kind, id)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let subject = SubjectRef::new(
        Namespace::parse(&namespace).map_err(|e| ApiError::bad_request(e.to_string()))?,
        SubjectKind::parse(&kind).map_err(|e| ApiError::bad_request(e.to_string()))?,
        id,
    )
    .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let store = state.store.read().expect("store lock");
    match store.graph().subject(&subject) {
        Some(node) => Ok(Json(node).into_response()),
        None => Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("unknown subject `{subject}`"),
        }),
    }
}

async fn get_events(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().expect("store lock");
    Json(serde_json::json!({ "events": store.log() }))
}

async fn post_event(
    State(state): State<AppState>,
    Json(event): Json<VEvent>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let mut store = state.store.write().expect("store lock");
    let appended = store.append(event)?;
    tracing::info!(sequence = appended.sequence, "event appended");
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "event_id": appended.event_id,
            "sequence": appended.sequence,
            "revision": appended.revision,
        })),
    ))
}

async fn post_patch(
    State(state): State<AppState>,
    Json(patch): Json<GraphPatch>,
) -> Result<Response, ApiError> {
    let mut store = state.store.write().expect("store lock");
    let outcome = store.propose_patch(patch)?;
    tracing::info!(status = ?outcome, "patch resolved");
    Ok(match outcome {
        PatchOutcome::Applied { .. } => (StatusCode::OK, Json(outcome)).into_response(),
        // Optimistic-concurrency and governance rejections surface as 409;
        // the rejection itself is durable (see GET /events).
        PatchOutcome::Rejected { .. } => (StatusCode::CONFLICT, Json(outcome)).into_response(),
    })
}
