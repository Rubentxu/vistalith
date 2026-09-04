//! `vistalithd` — the Vistalith server.
//!
//! Slice-1: in-memory event log + SWG projection over HTTP (subjects,
//! relations, events, patches).
//! Slice-3: conversation threads with one LLM provider behind
//! Vistalith-owned contracts (SPEC-007/008) and the C4 projection view.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use tokio::sync::RwLock;
use std::sync::Arc as StdArc;

use vistalith_agent_runtime::{
    ConversationEngine, ConversationError, FakeProvider, GrantStore, McpManager,
    McpServerConfig, ModelProvider, RuntimeProvider, ToolRegistry,
};
use vistalith_domain::{Namespace, SubjectKind, SubjectRef, VEvent};
use vistalith_graph::{
    GraphDiff, GraphPatch, GraphStore, PatchOutcome, StoreError, c4_view, canonical_graph_json,
};

/// Shared state: the durable log + its projection, and the conversation
/// runtime. A tokio lock is used because a turn awaits the provider while
/// holding the write guard.
#[derive(Clone)]
pub struct AppState {
    store: Arc<RwLock<GraphStore>>,
    runtime: Arc<RuntimeProvider>,
    /// Live MCP client connections (SPEC-009); tools project into the
    /// unified catalog on every turn.
    mcp: StdArc<McpManager>,
    /// Scoped temporary grants, shared across requests and turns
    /// (`agentic/TOOLS-PERMISSIONS.md`).
    grants: StdArc<GrantStore>,
}

impl AppState {
    pub fn new(store: GraphStore) -> Self {
        AppState::with_runtime(
            store,
            RuntimeProvider::Fake(FakeProvider::repeating(
                "This is the offline fake provider: wire a real provider with \
             --provider anthropic and an API key.",
            )),
        )
    }

    pub fn empty() -> Self {
        AppState::new(GraphStore::new())
    }

    pub fn with_runtime(store: GraphStore, runtime: RuntimeProvider) -> Self {
        AppState {
            store: Arc::new(RwLock::new(store)),
            runtime: Arc::new(runtime),
            mcp: StdArc::new(McpManager::new()),
            grants: StdArc::new(GrantStore::new()),
        }
    }

    pub fn with_parts(
        store: GraphStore,
        runtime: RuntimeProvider,
        mcp: StdArc<McpManager>,
        grants: StdArc<GrantStore>,
    ) -> Self {
        AppState {
            store: Arc::new(RwLock::new(store)),
            runtime: Arc::new(runtime),
            mcp,
            grants,
        }
    }

    fn engine(&self) -> ConversationEngine<Arc<RuntimeProvider>> {
        // The unified catalog: native tools plus every connected MCP
        // server's discovered tools (SPEC-009), one permission gate.
        let mut registry = ToolRegistry::native(self.grants.clone());
        for connection in self.mcp.connections() {
            registry.add_mcp(connection);
        }
        let mut engine = ConversationEngine::new(Arc::clone(&self.runtime)).with_tools(registry);
        if let Ok(system) = std::env::var("VISTALITH_SYSTEM_PROMPT") {
            engine = engine.with_system_prompt(system);
        }
        engine
    }

    pub fn mcp_manager(&self) -> &StdArc<McpManager> {
        &self.mcp
    }

    pub fn grant_store(&self) -> &StdArc<GrantStore> {
        &self.grants
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
        .route("/diff", get(get_diff))
        .route("/threads/{id}/fork", post(post_thread_fork))
        .route("/tools", get(get_tools))
        .route("/tools/{id}/grant", post(post_tool_grant))
        .route("/tools/{id}/revoke", post(post_tool_revoke))
        .route("/mcp/servers", get(get_mcp_servers).post(post_mcp_server))
        .route("/mcp/servers/{name}", delete(delete_mcp_server))
        .route("/subjects", get(get_subjects))
        .route("/subjects/{namespace}/{kind}/{id}", get(get_subject))
        .route("/events", get(get_events).post(post_event))
        .route("/patches", post(post_patch))
        .route("/threads", get(get_threads).post(post_thread))
        .route("/threads/{id}", get(get_thread))
        .route("/threads/{id}/messages", post(post_thread_message))
        .route("/intents", get(get_intents).post(post_intent))
        .route("/intents/{id}", get(get_intent))
        .route("/intents/{id}/promote", post(promote_intent))
        .route("/intents/{id}/discard", post(discard_intent))
        .route("/views/c4", get(get_c4_view))
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

impl From<ConversationError> for ApiError {
    fn from(err: ConversationError) -> Self {
        match &err {
            ConversationError::UnknownThread(_) => ApiError {
                status: StatusCode::NOT_FOUND,
                message: err.to_string(),
            },
            ConversationError::Model(_) => ApiError {
                status: StatusCode::BAD_GATEWAY,
                message: err.to_string(),
            },
            _ => ApiError::bad_request(err.to_string()),
        }
    }
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
    Json(serde_json::json!({
        "status": "ok",
        "service": "vistalithd",
        "graph_revision": store.graph().revision(),
        "events": store.log().len(),
        "provider": state.runtime.descriptor().to_string(),
    }))
}

async fn get_graph(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let store = state.store.read().await;
    match params.get("at_revision") {
        None => {
            let body: serde_json::Value =
                serde_json::from_str(&canonical_graph_json(store.graph())).expect("canonical JSON");
            Ok(Json(body).into_response())
        }
        // SPEC-011 time travel: render the graph as of an earlier revision.
        Some(raw) => {
            let revision: u64 = raw
                .parse()
                .map_err(|e| ApiError::bad_request(format!("invalid `at_revision`: {e}")))?;
            let graph = store.graph_at_revision(revision)?;
            let mut body: serde_json::Value =
                serde_json::from_str(&canonical_graph_json(&graph)).expect("canonical JSON");
            body["as_of_revision"] = serde_json::json!(graph.revision());
            Ok(Json(body).into_response())
        }
    }
}

async fn get_diff(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<GraphDiff>, ApiError> {
    let parse = |key: &str| -> Result<Option<u64>, ApiError> {
        match params.get(key) {
            None => Ok(None),
            Some(raw) => raw
                .parse()
                .map(Some)
                .map_err(|e| ApiError::bad_request(format!("invalid `{key}`: {e}"))),
        }
    };
    let from = parse("from")?.unwrap_or(0);
    let store = state.store.read().await;
    let to = match parse("to")? {
        Some(to) => to,
        None => store.graph().revision(),
    };
    let diff = store.diff_revisions(from, to)?;
    Ok(Json(diff))
}

async fn get_subjects(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
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

    let store = state.store.read().await;
    match store.graph().subject(&subject) {
        Some(node) => Ok(Json(node).into_response()),
        None => Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("unknown subject `{subject}`"),
        }),
    }
}

async fn get_events(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
    Json(serde_json::json!({ "events": store.log() }))
}

async fn post_event(
    State(state): State<AppState>,
    Json(event): Json<VEvent>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let mut store = state.store.write().await;
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
    let mut store = state.store.write().await;
    let outcome = store.propose_patch(patch)?;
    tracing::info!(status = ?outcome, "patch resolved");
    Ok(match outcome {
        PatchOutcome::Applied { .. } => (StatusCode::OK, Json(outcome)).into_response(),
        // Optimistic-concurrency and governance rejections surface as 409;
        // the rejection itself is durable (see GET /events).
        PatchOutcome::Rejected { .. } => (StatusCode::CONFLICT, Json(outcome)).into_response(),
    })
}

// --- Conversation (slice 3, SPEC-007) --------------------------------------

async fn post_thread(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let title = body
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("untitled thread")
        .to_owned();
    let engine = state.engine();
    let mut store = state.store.write().await;
    let thread = engine.start_thread(&mut store, title)?;
    tracing::info!(thread = %thread, "thread started");
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "thread": thread.to_string() })),
    ))
}

fn thread_summary(store: &GraphStore, thread: &SubjectRef) -> serde_json::Value {
    let node = store.graph().subject(thread).expect("thread node exists");
    serde_json::json!({
        "thread": thread.to_string(),
        "title": node.properties.get("title").cloned().unwrap_or(serde_json::json!("untitled")),
        "turns": node.properties.get("turns").cloned().unwrap_or(serde_json::json!(0)),
        "last_model": node.properties.get("last_model").cloned(),
        "forked_from": node.properties.get("forked_from").cloned(),
    })
}

async fn get_threads(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
    let threads: Vec<serde_json::Value> = store
        .graph()
        .subjects_of_kind(&SubjectKind::Thread)
        .map(|node| thread_summary(&store, &node.subject))
        .collect();
    Json(serde_json::json!({ "threads": threads }))
}

fn parse_thread(id: &str) -> Result<SubjectRef, ApiError> {
    SubjectRef::new(Namespace::Agentic, SubjectKind::Thread, id)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

async fn get_thread(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let thread = parse_thread(&id)?;
    let store = state.store.read().await;
    if store.graph().subject(&thread).is_none() {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("unknown thread `{thread}`"),
        });
    }
    let messages: Vec<serde_json::Value> = store
        .graph()
        .children(&thread)
        .into_iter()
        .map(|node| {
            serde_json::json!({
                "message": node.subject.to_string(),
                "role": node.properties.get("role").cloned().unwrap_or(serde_json::json!("user")),
                "content": node.properties.get("content").cloned().unwrap_or(serde_json::json!("")),
                "turn": node.properties.get("turn").cloned().unwrap_or(serde_json::json!(0)),
                "forked_of": node.properties.get("forked_of").cloned(),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({
        "thread": thread_summary(&store, &thread),
        "messages": messages,
    })))
}

async fn post_thread_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let thread = parse_thread(&id)?;
    let content = body
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("missing `content` string".to_owned()))?
        .to_owned();
    let engine = state.engine();
    let mut store = state.store.write().await;
    let reply = engine
        .send_user_message(&mut store, &thread, content)
        .await?;
    tracing::info!(turn = reply.turn, thread = %thread, "turn completed");
    Ok(Json(serde_json::json!({
        "thread": thread.to_string(),
        "message": reply.message.to_string(),
        "turn": reply.turn,
        "content": reply.content,
        "usage": {
            "input_tokens": reply.usage.input_tokens,
            "output_tokens": reply.usage.output_tokens,
            "total_tokens": reply.usage.total_tokens,
        },
    })))
}

// --- Fork / diff / time travel (slice 5, SPEC-011) --------------------------

async fn post_thread_fork(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let source = parse_thread(&id)?;
    let up_to_turn = body.get("up_to_turn").and_then(|v| v.as_u64());
    let note = body
        .get("note")
        .and_then(|v| v.as_str())
        .map(std::borrow::ToOwned::to_owned);
    let engine = state.engine();
    let mut store = state.store.write().await;
    let forked = engine.fork_thread(&mut store, &source, up_to_turn, note)?;
    tracing::info!(
        fork = %forked.fork,
        copied = forked.copied_events,
        "thread forked"
    );
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "fork": forked.fork.to_string(),
            "source": forked.source.to_string(),
            "up_to_turn": forked.up_to_turn,
            "copied_events": forked.copied_events,
        })),
    ))
}

// --- Unified tool catalog + MCP (slice 6, SPEC-009) -------------------------

/// One catalog row as seen over the API: descriptor + current permission
/// decision + remaining scoped grant.
fn tool_row(
    grants: &GrantStore,
    descriptor: &vistalith_agent_runtime::ToolDescriptor,
) -> serde_json::Value {
    let decision = match grants.is_denied(&descriptor.id) {
        true => vistalith_agent_runtime::PermissionDecision::Deny,
        false => match descriptor.consequence {
            vistalith_agent_runtime::Consequence::ReadOnly => {
                vistalith_agent_runtime::PermissionDecision::Allow
            }
            _ if grants.remaining(&descriptor.id) > 0 => {
                vistalith_agent_runtime::PermissionDecision::Allow
            }
            _ => vistalith_agent_runtime::PermissionDecision::Ask,
        },
    };
    serde_json::json!({
        "id": descriptor.id,
        "description": descriptor.description,
        "consequence": descriptor.consequence,
        "source": descriptor.source,
        "parameters": descriptor.parameters,
        "permission": decision,
        "grant_remaining": grants.remaining(&descriptor.id),
    })
}

/// Builds the unified catalog exactly as a turn would see it.
fn unified_catalog(state: &AppState) -> ToolRegistry {
    let mut registry = ToolRegistry::native(state.grants.clone());
    for connection in state.mcp.connections() {
        registry.add_mcp(connection);
    }
    registry
}

async fn get_tools(State(state): State<AppState>) -> Json<serde_json::Value> {
    let registry = unified_catalog(&state);
    let rows: Vec<serde_json::Value> = registry
        .descriptors()
        .iter()
        .map(|d| tool_row(&state.grants, d))
        .collect();
    Json(serde_json::json!({ "tools": rows, "grants": state.grants.all() }))
}

async fn post_tool_grant(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let calls = body.get("calls").and_then(|v| v.as_u64()).unwrap_or(1);
    if calls == 0 || calls > 1000 {
        return Err(ApiError::bad_request(
            "`calls` must be between 1 and 1000".to_owned(),
        ));
    }
    let known = unified_catalog(&state).get(&id).is_some();
    if !known {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("unknown tool `{id}`"),
        });
    }
    if state.grants.is_denied(&id) {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            message: format!("tool `{id}` is denied by policy; grants are ignored"),
        });
    }
    let grant = state.grants.grant(&id, calls as u32);
    tracing::info!(tool = %id, calls, "scoped grant created");
    Ok(Json(serde_json::json!({ "tool": grant.tool, "remaining": grant.remaining })))
}

async fn post_tool_revoke(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let removed = state.grants.revoke(&id);
    Json(serde_json::json!({ "tool": id, "revoked": removed }))
}

async fn get_mcp_servers(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "servers": state.mcp.status() }))
}

async fn post_mcp_server(
    State(state): State<AppState>,
    Json(config): Json<McpServerConfig>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    if state.mcp.get(&config.name).is_some() {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            message: format!("MCP server `{}` is already registered", config.name),
        });
    }
    config
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let status = state
        .mcp
        .register(config)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    tracing::info!(server = %status.name, tools = status.tools, "MCP server registered");
    Ok((StatusCode::CREATED, Json(serde_json::to_value(status).expect("status serialization"))))
}

async fn delete_mcp_server(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(connection) = state.mcp.take(&name) else {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("unknown MCP server `{name}`"),
        });
    };
    // Revoke that server's grants: the tools disappear from the catalog.
    for descriptor in connection.entries().iter().map(|e| &e.descriptor) {
        state.grants.revoke(&descriptor.id);
        state.grants.set_denied(&descriptor.id, false);
    }
    connection.shutdown().await;
    Ok(Json(serde_json::json!({ "removed": name })))
}

// --- C4 projection (slice 3, IMPLEMENT-NOW item 12) ------------------------

async fn get_c4_view(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
    let view = c4_view(store.graph());
    Json(serde_json::json!({
        "revision": view.revision,
        "systems": view.systems,
        "containers": view.containers,
        "components": view.components,
        "relationships": view.relationships,
    }))
}

// --- Visual intents (slice 4, SPEC-006) -------------------------------------

use vistalith_agent_runtime::{
    IntentError, Promotion, discard_intent as discard_intent_op, draft_intent,
    promote_intent as promote_intent_op,
};
use vistalith_domain::Actor;

impl From<IntentError> for ApiError {
    fn from(err: IntentError) -> Self {
        match &err {
            IntentError::UnknownIntent(_) | IntentError::UnknownTarget(_) => ApiError {
                status: StatusCode::NOT_FOUND,
                message: err.to_string(),
            },
            _ => ApiError::bad_request(err.to_string()),
        }
    }
}

fn parse_intent(id: &str) -> Result<SubjectRef, ApiError> {
    SubjectRef::new(Namespace::Visual, SubjectKind::VisualProposal, id)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

fn body_actor(body: &serde_json::Value) -> Result<Actor, ApiError> {
    let raw = body
        .get("actor")
        .and_then(|v| v.as_str())
        .unwrap_or("user:api");
    Actor::new(raw).map_err(|e| ApiError::bad_request(e.to_string()))
}

fn intent_summary(store: &GraphStore, intent: &SubjectRef) -> Option<serde_json::Value> {
    let node = store.graph().subject(intent)?;
    let target = store
        .graph()
        .outgoing(intent)
        .find(|f| f.relation.kind.as_str() == "proposes_change_to")
        .map(|f| f.relation.to.to_string());
    let base_revision = node
        .properties
        .get("base_revision")
        .cloned()
        .unwrap_or(serde_json::json!(0));
    Some(serde_json::json!({
        "intent": intent.to_string(),
        "target": target,
        "gesture": node.properties.get("gesture").cloned().unwrap_or(serde_json::json!("unknown")),
        "status": node.properties.get("status").cloned().unwrap_or(serde_json::json!("draft")),
        "base_revision": base_revision,
        "stale": base_revision.as_u64() != Some(store.graph().revision()),
    }))
}

async fn post_intent(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let target = SubjectRef::parse(
        body.get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::bad_request("missing `target` identity string".into()))?,
    )
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let gesture = body
        .get("gesture")
        .and_then(|v| v.as_str())
        .unwrap_or("annotate")
        .to_owned();
    let change = body
        .get("change")
        .cloned()
        .ok_or_else(|| ApiError::bad_request("missing `change` payload".into()))?;
    let reason = body
        .get("reason")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let actor = body_actor(&body)?;

    let engine_actor = actor;
    let mut store = state.store.write().await;
    let intent = draft_intent(&mut store, &target, gesture, change, reason, &engine_actor)?;
    tracing::info!(intent = %intent, "intent drafted");
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "intent": intent.to_string(),
            "base_revision": store.graph().revision(),
        })),
    ))
}

async fn get_intents(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
    let intents: Vec<serde_json::Value> = store
        .graph()
        .subjects_of_kind(&SubjectKind::VisualProposal)
        .filter_map(|node| intent_summary(&store, &node.subject))
        .collect();
    Json(serde_json::json!({ "intents": intents }))
}

async fn get_intent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let intent = parse_intent(&id)?;
    let store = state.store.read().await;
    let summary = intent_summary(&store, &intent).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("unknown intent `{intent}`"),
    })?;
    let node = store.graph().subject(&intent).expect("checked above");
    Ok(Json(serde_json::json!({
        "summary": summary,
        "change": node.properties.get("change").cloned().unwrap_or(serde_json::Value::Null),
        "current_revision": store.graph().revision(),
    })))
}

async fn promote_intent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, ApiError> {
    let intent = parse_intent(&id)?;
    let actor = body_actor(&body)?;
    let mut store = state.store.write().await;
    let outcome = promote_intent_op(&mut store, &intent, &actor)?;
    tracing::info!(intent = %intent, outcome = ?outcome, "intent promoted");
    Ok(match outcome {
        Promotion::Applied { revision } => (
            StatusCode::OK,
            Json(serde_json::json!({ "outcome": "applied", "revision": revision })),
        ),
        Promotion::RoutedToSddkGovernance { subject } => (
            StatusCode::OK,
            Json(serde_json::json!({
                "outcome": "sddk-governed",
                "subject": subject.to_string(),
                "note": "SDDK-owned truth: convert this semantic change proposal into \
                         SDDK-governed work through the SDDK flow."
            })),
        ),
        Promotion::Stale {
            current_revision,
            base_revision,
        } => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "outcome": "stale",
                "current_revision": current_revision,
                "base_revision": base_revision,
            })),
        ),
        Promotion::RejectedLocally { reason } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "outcome": "rejected", "reason": reason })),
        ),
    }
    .into_response())
}

async fn discard_intent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let intent = parse_intent(&id)?;
    let actor = body_actor(&body)?;
    let reason = body
        .get("reason")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let mut store = state.store.write().await;
    discard_intent_op(&mut store, &intent, reason, &actor)?;
    Ok(Json(serde_json::json!({ "outcome": "discarded" })))
}
