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
    ConversationEngine, ConversationError, FakeProvider, FrameError, FrameOutcome, FrameSpec,
    GrantStore, McpManager, McpServerConfig, ModelProvider, RuntimeProvider, ToolRegistry,
    close_frame, draft_intent, finish_agent_run, frame_system_prompt,
    promote_intent_with_bridge, run_frame_turn, start_agent_frame, start_frame,
};
use vistalith_domain::{
    Namespace, RelationKind, SubjectKind, SubjectRef, UatCheckRecorded, UatVerdict, VEvent,
};
use vistalith_graph::{
    AlgorithmGraph, ContextRequest, DecisionsLens, GraphDiff, GraphPatch, GraphStore,
    PatchOutcome, StoreError, append_and_react, c4_view, canonical_graph_json, why_path,
};
use vistalith_sddk_bridge::{
    FocusAnswer, FocusTest, PullUpEvaluation, SddkBridge,
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
    /// Reactive behaviors dispatched on live event appends (SPEC-003).
    /// Shared, because the built-in set is process-wide.
    behaviors: StdArc<Vec<Box<dyn vistalith_graph::Behavior>>>,
    /// The governed SDDK promotion bridge (SPK-012), when configured
    /// (`--sddk-ledger/--sddk-workflow/--sddk-project`).
    sddk_bridge: Option<StdArc<SddkBridge>>,
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
            behaviors: StdArc::new(vistalith_graph::builtin_behaviors()),
            sddk_bridge: None,
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
            behaviors: StdArc::new(vistalith_graph::builtin_behaviors()),
            sddk_bridge: None,
        }
    }

    /// Installs the governed SDDK promotion bridge.
    pub fn with_sddk_bridge(mut self, bridge: SddkBridge) -> Self {
        self.sddk_bridge = Some(StdArc::new(bridge));
        self
    }

    pub fn sddk_bridge(&self) -> Option<&StdArc<SddkBridge>> {
        self.sddk_bridge.as_ref()
    }

    fn unified_catalog(&self) -> ToolRegistry {
        // The unified catalog: native tools plus every connected MCP
        // server's discovered tools (SPEC-009), one permission gate.
        let mut registry = ToolRegistry::native(self.grants.clone());
        for connection in self.mcp.connections() {
            registry.add_mcp(connection);
        }
        registry
    }

    fn engine(&self) -> ConversationEngine<Arc<RuntimeProvider>> {
        let registry = self.unified_catalog();
        let system = std::env::var("VISTALITH_SYSTEM_PROMPT").ok();
        self.engine_with(registry, system)
    }

    fn engine_with(
        &self,
        registry: ToolRegistry,
        system: Option<String>,
    ) -> ConversationEngine<Arc<RuntimeProvider>> {
        let mut engine = ConversationEngine::new(Arc::clone(&self.runtime)).with_tools(registry);
        if let Some(system) = system {
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
        .route("/mcp/servers/{name}/health", get(get_mcp_server_health))
        .route("/mcp/servers/{name}/refresh", post(post_mcp_server_refresh))
        .route("/mcp/servers/{name}/disable", post(post_mcp_server_disable))
        .route("/mcp/servers/{name}/enable", post(post_mcp_server_enable))
        .route("/agents", get(get_agents).post(post_agent))
        .route("/agents/{id}/run", post(post_agent_run))
        .route("/frames", get(get_frames).post(post_frame))
        .route("/frames/{id}", get(get_frame))
        .route("/frames/{id}/turns", post(post_frame_turn))
        .route("/frames/{id}/close", post(post_frame_close))
        .route("/subjects", get(get_subjects))
        .route("/subjects/{namespace}/{kind}/{id}", get(get_subject))
        .route("/events", get(get_events).post(post_event))
        .route("/patches", post(post_patch))
        .route("/threads", get(get_threads).post(post_thread))
        .route("/threads/{id}", get(get_thread))
        .route("/threads/{id}/messages", post(post_thread_message))
        .route("/threads/{id}/messages/stream", post(post_thread_message_stream))
        .route("/intents", get(get_intents).post(post_intent))
        .route("/intents/{id}", get(get_intent))
        .route("/intents/{id}/promote", post(promote_intent))
        .route("/intents/{id}/discard", post(discard_intent))
        .route("/views/c4", get(get_c4_view))
        .route("/views/context", post(post_context_view))
        .route("/algorithms/impact/{namespace}/{kind}/{id}", get(get_impact))
        .route("/algorithms/path", get(get_path))
        .route("/algorithms/cycles", get(get_cycles))
        .route("/sddk/receipts", get(get_sddk_receipts))
        .route("/sddk/pull-up", post(post_sddk_pull_up))
        .route("/sddk/sync", post(post_sddk_sync))
        .route("/why/{namespace}/{kind}/{id}", get(get_why))
        .route("/lens/decisions", get(get_decisions_lens))
        .route("/uat/checks", post(post_uat_check))
        .route("/lens/uat", get(get_uat_lens))
        .route("/canvas/subjects", post(post_canvas_subject))
        .route("/canvas/subjects", get(get_canvas_subjects))
        .route("/canvas/subjects/{ns}/{kind}/{id}/promote", post(post_canvas_promote))
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

impl From<FrameError> for ApiError {
    fn from(err: FrameError) -> Self {
        match &err {
            FrameError::UnknownFrame(_) | FrameError::UnknownSubject(_) => ApiError {
                status: StatusCode::NOT_FOUND,
                message: err.to_string(),
            },
            FrameError::Closed(_)
            | FrameError::TurnsExhausted(_, _)
            | FrameError::BudgetExhausted(_, _) => ApiError {
                status: StatusCode::CONFLICT,
                message: err.to_string(),
            },
            FrameError::NoThread(_) => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: err.to_string(),
            },
            FrameError::Store(inner) => ApiError::from(inner.clone()),
            FrameError::Conversation(inner) => ApiError::from(inner.clone()),
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
    // SPEC-003: live appends dispatch the reactive behaviors; their
    // advisories are appended with causation_id -> this event.
    let outcome = {
        let behaviors = state.behaviors.clone();
        append_and_react(&mut store, event, &behaviors)?
    };
    let appended = outcome.appended;
    tracing::info!(sequence = appended.sequence, advisories = outcome.advisories, "event appended");
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "event_id": appended.event_id,
            "sequence": appended.sequence,
            "revision": appended.revision,
            "advisories_raised": outcome.advisories,
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

/// POST /threads/{id}/messages/stream — Server-Sent Events turn:
/// `event: delta` {text} as the model streams, then
/// `event: done` {turn, message, content, usage}. Durability is identical
/// to the non-streamed endpoint: events append at the same points.
/// POST /threads/{id}/messages/stream — Server-Sent Events turn:
/// `event: delta` {text} frames as the model streams, then a terminal
/// `event: done` (or `event: error`) frame. Durability is identical to the
/// non-streamed endpoint: the same events append at the same points.
async fn post_thread_message_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, ApiError> {
    let thread = parse_thread(&id)?;
    let content = body
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("missing `content` string".to_owned()))?
        .to_owned();

    // Fail fast (unknown thread) before the stream opens.
    {
        let store = state.store.read().await;
        if store.graph().subject(&thread).is_none() {
            return Err(ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("unknown thread `{thread}`"),
            });
        }
    }

    let (frame_tx, frame_rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(32);
    let store = StdArc::clone(&state.store);
    let engine = state.engine();

    // The turn runs on its own task (it holds the write guard for the whole
    // streamed turn) and hands deltas to a forwarder, so SSE frames flow
    // while the model streams.
    let (deltas_tx, mut deltas_rx) = tokio::sync::mpsc::channel::<String>(16);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel::<
        Result<vistalith_agent_runtime::ThreadReply, String>,
    >();
    let turn_thread = thread.clone();
    tokio::spawn(async move {
        let mut store = store.write().await;
        let result = engine
            .send_user_message_streaming(&mut store, &turn_thread, content, deltas_tx)
            .await;
        let _ = result_tx.send(result.map_err(|e| e.to_string()));
    });

    // Forwarder: deltas live; the terminal frame comes after the deltas
    // channel closes (the turn returned).
    let forwarder_tx = frame_tx.clone();
    tokio::spawn(async move {
        while let Some(delta) = deltas_rx.recv().await {
            let frame = format!("event: delta\ndata: {}\n\n", sse_data(&delta));
            if forwarder_tx.send(Ok(frame)).await.is_err() {
                return; // client disconnected
            }
        }
        let frame = match result_rx.await {
            Ok(Ok(reply)) => format!(
                "event: done\ndata: {}\n\n",
                sse_data(
                    &serde_json::to_string(&serde_json::json!({
                        "turn": reply.turn,
                        "message": reply.message.to_string(),
                        "content": reply.content,
                        "usage": {
                            "input_tokens": reply.usage.input_tokens,
                            "output_tokens": reply.usage.output_tokens,
                            "total_tokens": reply.usage.total_tokens,
                        },
                    }))
                    .expect("done frame serialization")
                )
            ),
            Ok(Err(err)) => format!(
                "event: error\ndata: {}\n\n",
                sse_data(&serde_json::json!(err).to_string())
            ),
            Err(_) => "event: error\ndata: turn task dropped\n\n".to_owned(),
        };
        let _ = forwarder_tx.send(Ok(frame)).await;
    });

    let stream = futures::stream::unfold(frame_rx, |mut rx| async move {
        rx.recv().await.map(|frame| (frame, rx))
    });
    Ok((
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        axum::body::Body::from_stream(stream),
    )
        .into_response())
}

/// SSE data frames cannot contain raw newlines; escape them and let the
/// client unescape.
fn sse_data(raw: &str) -> String {
    raw.replace('\n', "\\n").replace('\r', "\\r")
}

// --- Fork / diff / time travel (slice 5, SPEC-011) --------------------------// --- Fork / diff / time travel (slice 5, SPEC-011) --------------------------

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
    drop(connection); // dropping the last Arc closes the transport
    Ok(Json(serde_json::json!({ "removed": name })))
}

// --- Reactive behaviors / algorithms / context view (slice 7) ---------------

async fn post_context_view(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Wire format: roots as `ns:kind:id` identity strings (the API-wide
    // convention), relation kinds as their wire names.
    let parse_root = |raw: &str| SubjectRef::parse(raw).map_err(|e| ApiError::bad_request(e.to_string()));
    let roots_value = body
        .get("roots")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ApiError::bad_request("missing `roots` array".to_owned()))?;
    let mut roots = Vec::new();
    for root in roots_value {
        let raw = root
            .as_str()
            .ok_or_else(|| ApiError::bad_request("`roots` entries must be identity strings".to_owned()))?;
        roots.push(parse_root(raw)?);
    }
    if roots.is_empty() {
        return Err(ApiError::bad_request(
            "`roots` must contain at least one subject".to_owned(),
        ));
    }
    let relations = match body.get("relations") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => {
            let raw_list = value
                .as_array()
                .ok_or_else(|| ApiError::bad_request("`relations` must be an array".to_owned()))?;
            let mut kinds = Vec::new();
            for raw in raw_list {
                let raw = raw
                    .as_str()
                    .ok_or_else(|| ApiError::bad_request("`relations` entries must be strings".to_owned()))?;
                kinds.push(
                    RelationKind::parse(raw).map_err(|e| ApiError::bad_request(e.to_string()))?,
                );
            }
            Some(kinds)
        }
    };
    let parse_flag = |key: &str| -> Result<bool, ApiError> {
        match body.get(key) {
            None | Some(serde_json::Value::Null) => Ok(false),
            Some(value) => value
                .as_bool()
                .ok_or_else(|| ApiError::bad_request(format!("`{key}` must be a boolean"))),
        }
    };
    let max_depth = match body.get("max_depth") {
        None | Some(serde_json::Value::Null) => 2,
        Some(value) => value
            .as_u64()
            .and_then(|v| u8::try_from(v).ok())
            .ok_or_else(|| ApiError::bad_request("`max_depth` must be 0..=255".to_owned()))?,
    };
    let token_budget = match body.get("token_budget") {
        None | Some(serde_json::Value::Null) => 8_000,
        Some(value) => value
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| ApiError::bad_request("`token_budget` must be a positive integer".to_owned()))?,
    };
    let request = ContextRequest {
        roots,
        relations,
        max_depth,
        include_derived: parse_flag("include_derived")?,
        include_advisory: parse_flag("include_advisory")?,
        recency_cutoff: None,
        token_budget,
    };
    let store = state.store.read().await;
    let view = vistalith_graph::build_context_view(&store, &request);
    Ok(Json(serde_json::to_value(view).expect("view serialization")))
}

fn parse_kinds(raw: Option<String>) -> Result<Option<Vec<RelationKind>>, ApiError> {
    match raw {
        None => Ok(None),
        Some(raw) => {
            let mut kinds = Vec::new();
            for part in raw.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                kinds.push(
                    RelationKind::parse(part)
                        .map_err(|e| ApiError::bad_request(e.to_string()))?,
                );
            }
            Ok(Some(kinds))
        }
    }
}

async fn get_impact(
    State(state): State<AppState>,
    Path((namespace, kind, id)): Path<(String, String, String)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let subject = SubjectRef::new(
        Namespace::parse(&namespace).map_err(|e| ApiError::bad_request(e.to_string()))?,
        SubjectKind::parse(&kind).map_err(|e| ApiError::bad_request(e.to_string()))?,
        id,
    )
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let kinds = parse_kinds(params.get("kinds").cloned())?;
    let full = params.get("full").map(|v| v == "true").unwrap_or(false);
    let store = state.store.read().await;
    let snapshot = AlgorithmGraph::extract(store.graph(), kinds.as_deref());
    if full {
        // Full analysis (visual/IMPACT.md): tests/evidence/decisions/
        // unknown sections explicit.
        let analysis = snapshot
            .impact_analysis(&subject, full)
            .ok_or_else(|| ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("unknown subject `{subject}`"),
            })?;
        return Ok(Json(
            serde_json::to_value(analysis).expect("impact serialization"),
        ));
    }
    let report = snapshot.impact_of(&subject).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("unknown subject `{subject}`"),
    })?;
    Ok(Json(serde_json::to_value(report).expect("impact serialization")))
}

async fn get_path(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let parse_ref = |raw: &str| -> Result<SubjectRef, ApiError> {
        SubjectRef::parse(raw).map_err(|e| ApiError::bad_request(e.to_string()))
    };
    let from = parse_ref(
        params
            .get("from")
            .ok_or_else(|| ApiError::bad_request("missing `from`".to_owned()))?,
    )?;
    let to = parse_ref(
        params
            .get("to")
            .ok_or_else(|| ApiError::bad_request("missing `to`".to_owned()))?,
    )?;
    let kinds = parse_kinds(params.get("kinds").cloned())?;
    let store = state.store.read().await;
    let snapshot = AlgorithmGraph::extract(store.graph(), kinds.as_deref());
    let report = snapshot.shortest_path(&from, &to).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("no path from `{from}` to `{to}`"),
    })?;
    Ok(Json(serde_json::to_value(report).expect("path serialization")))
}

async fn get_cycles(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let kinds = parse_kinds(params.get("kinds").cloned())?;
    let store = state.store.read().await;
    let snapshot = AlgorithmGraph::extract(store.graph(), kinds.as_deref());
    let report = snapshot.cycles();
    Ok(Json(serde_json::to_value(report).expect("cycle serialization")))
}

// --- Agents & frames (slice 8, PATTERNS-VIEWS-FRAMES + AGENTS-DELEGATION) ---

async fn post_agent(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let role = body
        .get("role")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("missing `role` string".to_owned()))?
        .to_owned();
    let instructions = body
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let model = match body.get("model").and_then(|v| v.as_str()) {
        None => None,
        Some(raw) => {
            let (provider, name) = raw
                .split_once('/')
                .ok_or_else(|| ApiError::bad_request("`model` must be `provider/model`".to_owned()))?;
            Some(vistalith_domain::ModelDescriptor::new(provider, name))
        }
    };
    let string_list = |key: &str| -> Result<Vec<String>, ApiError> {
        match body.get(key) {
            None | Some(serde_json::Value::Null) => Ok(Vec::new()),
            Some(value) => value
                .as_array()
                .ok_or_else(|| ApiError::bad_request(format!("`{key}` must be an array")))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| ApiError::bad_request(format!("`{key}` entries must be strings")))
                })
                .collect(),
        }
    };
    let tools = string_list("tools")?;
    let expected_outputs = string_list("expected_outputs")?;
    let budget_turns = match body.get("budget_turns") {
        None | Some(serde_json::Value::Null) => None::<u32>,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| ApiError::bad_request("`budget_turns` must be 0..=2^32".to_owned()))?,
        ),
    };

    let mut store = state.store.write().await;
    let agent = vistalith_agent_runtime::define_agent(
        &mut store,
        role,
        instructions,
        model,
        tools,
        budget_turns,
        expected_outputs,
    )?;
    tracing::info!(agent = %agent, "agent defined");
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "agent": agent.to_string() }))))
}

async fn post_agent_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let agent = SubjectRef::new(Namespace::Agentic, SubjectKind::Agent, id)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let goal = body
        .get("goal")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("missing `goal` string".to_owned()))?
        .to_owned();
    let mut subjects = Vec::new();
    if let Some(list) = body.get("subjects").and_then(|v| v.as_array()) {
        for raw in list {
            subjects.push(
                raw.as_str()
                    .and_then(|raw| SubjectRef::parse(raw).ok())
                    .ok_or_else(|| {
                        ApiError::bad_request("`subjects` entries must be identity strings".to_owned())
                    })?,
            );
        }
    }
    let token_budget = body.get("token_budget").and_then(|v| v.as_u64()).unwrap_or(8_000);

    let mut store = state.store.write().await;
    let (frame, permitted) = start_agent_frame(
        &mut store,
        &agent,
        goal.clone(),
        subjects,
        5,
        token_budget,
    )?;
    let prompt = frame_system_prompt(&store, &frame)?;
    let registry = unified_catalog(&state).restricted_to(&permitted);
    let engine = state.engine_with(registry, Some(prompt));

    // Run one bounded turn, then close the frame and record the structured
    // outputs (AGENTS-DELEGATION.md "Outputs").
    let report = run_frame_turn(&mut store, &frame, &engine, goal).await?;
    let findings = vec![format!(
        "completed {} turn(s), {} tokens",
        report.turns_used, report.used_tokens
    )];
    let run = finish_agent_run(
        &mut store,
        &frame,
        &agent,
        "completed",
        findings,
        Vec::new(),
        Vec::new(),
    )
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "run": run.to_string(),
            "frame": frame.to_string(),
            "turns": report.turns_used,
            "used_tokens": report.used_tokens,
            "auto_closed": report.auto_closed,
        })),
    ))
}

async fn get_agents(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
    let agents: Vec<serde_json::Value> = store
        .graph()
        .subjects_of_kind(&SubjectKind::Agent)
        .map(|node| {
            serde_json::json!({
                "agent": node.subject.to_string(),
                "role": node.properties.get("role").cloned().unwrap_or(serde_json::json!("")),
                "instructions": node.properties.get("instructions").cloned().unwrap_or(serde_json::json!("")),
                "tools": node.properties.get("tools").cloned().unwrap_or(serde_json::json!([])),
                "budget_turns": node.properties.get("budget_turns").cloned(),
            })
        })
        .collect();
    Json(serde_json::json!({ "agents": agents }))
}

fn frame_summary(node: &vistalith_graph::SubjectNode) -> serde_json::Value {
    serde_json::json!({
        "frame": node.subject.to_string(),
        "goal": node.properties.get("goal").cloned().unwrap_or(serde_json::json!("")),
        "status": node.properties.get("status").cloned().unwrap_or(serde_json::json!("open")),
        "turns": node.properties.get("turns").cloned().unwrap_or(serde_json::json!(0)),
        "max_turns": node.properties.get("max_turns").cloned().unwrap_or(serde_json::json!(0)),
        "used_tokens": node.properties.get("used_tokens").cloned().unwrap_or(serde_json::json!(0)),
        "token_budget": node.properties.get("token_budget").cloned().unwrap_or(serde_json::json!(0)),
        "permitted_tools": node.properties.get("permitted_tools").cloned().unwrap_or(serde_json::json!([])),
        "outcome": node.properties.get("outcome").cloned(),
        "summary": node.properties.get("summary").cloned(),
    })
}

async fn get_frames(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
    let frames: Vec<serde_json::Value> = store
        .graph()
        .subjects_of_kind(&SubjectKind::Frame)
        .map(frame_summary)
        .collect();
    Json(serde_json::json!({ "frames": frames }))
}

fn parse_frame(id: &str) -> Result<SubjectRef, ApiError> {
    SubjectRef::new(Namespace::Agentic, SubjectKind::Frame, id)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

async fn get_frame(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let frame = parse_frame(&id)?;
    let store = state.store.read().await;
    if store.graph().subject(&frame).is_none() {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("unknown frame `{frame}`"),
        });
    }
    let summary = store
        .graph()
        .subject(&frame)
        .map(frame_summary)
        .expect("checked above");
    let messages: Vec<serde_json::Value> =
        match vistalith_agent_runtime::frame_thread(&store, &frame) {
            Ok(thread) => store
                .graph()
                .children(&thread)
                .into_iter()
                .map(|node| {
                    serde_json::json!({
                        "message": node.subject.to_string(),
                        "role": node.properties.get("role").cloned().unwrap_or(serde_json::json!("user")),
                        "content": node.properties.get("content").cloned().unwrap_or(serde_json::json!("")),
                        "turn": node.properties.get("turn").cloned().unwrap_or(serde_json::json!(0)),
                    })
                })
                .collect(),
            Err(_) => Vec::new(),
        };
    Ok(Json(serde_json::json!({ "frame": summary, "messages": messages })))
}

async fn post_frame(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let goal = body
        .get("goal")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("missing `goal` string".to_owned()))?
        .to_owned();
    let agent = match body.get("agent").and_then(|v| v.as_str()) {
        None => None,
        Some(raw) => Some(
            SubjectRef::parse(raw).map_err(|e| ApiError::bad_request(e.to_string()))?,
        ),
    };
    let mut subjects = Vec::new();
    if let Some(list) = body.get("subjects").and_then(|v| v.as_array()) {
        for raw in list {
            let raw = raw
                .as_str()
                .ok_or_else(|| ApiError::bad_request("`subjects` entries must be identity strings".to_owned()))?;
            subjects.push(SubjectRef::parse(raw).map_err(|e| ApiError::bad_request(e.to_string()))?);
        }
    }
    let permitted_tools = match body.get("permitted_tools") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(value) => value
            .as_array()
            .ok_or_else(|| ApiError::bad_request("`permitted_tools` must be an array".to_owned()))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| ApiError::bad_request("`permitted_tools` entries must be strings".to_owned()))
            })
            .collect::<Result<Vec<String>, ApiError>>()?,
    };
    let max_turns = body
        .get("max_turns")
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(5);
    let token_budget = body.get("token_budget").and_then(|v| v.as_u64()).unwrap_or(8_000);

    let mut store = state.store.write().await;
    let frame = start_frame(
        &mut store,
        FrameSpec {
            goal,
            agent,
            subjects,
            permitted_tools,
            max_turns,
            token_budget,
        },
    )?;
    tracing::info!(frame = %frame, "frame started");
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "frame": frame.to_string(),
            "thread": vistalith_agent_runtime::frame_thread(&store, &frame)
                .map(|t| t.to_string())
                .unwrap_or_default(),
        })),
    ))
}

async fn post_frame_turn(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let frame = parse_frame(&id)?;
    let content = body
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("missing `content` string".to_owned()))?
        .to_owned();

    // Bounds and prompt come from the durable frame; the catalog is the
    // unified one restricted to the frame's permitted tools.
    let (system, allowed) = {
        let store = state.store.read().await;
        let system = frame_system_prompt(&store, &frame)?;
        let allowed = store
            .graph()
            .subject(&frame)
            .and_then(|node| node.properties.get("permitted_tools").cloned())
            .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
            .unwrap_or_default();
        (Some(system), allowed)
    };
    let registry = unified_catalog(&state).restricted_to(&allowed);
    let engine = state.engine_with(registry, system);

    let mut store = state.store.write().await;
    let report = run_frame_turn(&mut store, &frame, &engine, content).await?;
    tracing::info!(frame = %frame, turn = report.turn, "frame turn completed");
    Ok(Json(serde_json::json!({
        "frame": report.frame.to_string(),
        "turn": report.turn,
        "content": {
            "turns_used": report.turns_used,
            "used_tokens": report.used_tokens,
        },
        "auto_closed": report.auto_closed,
    })))
}

async fn post_frame_close(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let frame = parse_frame(&id)?;
    let outcome = match body
        .get("outcome")
        .and_then(|v| v.as_str())
        .unwrap_or("completed")
    {
        "completed" => FrameOutcome::Completed,
        "aborted" => FrameOutcome::Aborted,
        other => {
            return Err(ApiError::bad_request(format!(
                "unknown outcome `{other}` (completed | aborted)"
            )));
        }
    };
    let summary = body
        .get("summary")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let mut store = state.store.write().await;
    close_frame(&mut store, &frame, outcome, summary)?;
    let node = store
        .graph()
        .subject(&frame)
        .ok_or_else(|| ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("unknown frame `{frame}`"),
        })?;
    Ok(Json(frame_summary(node)))
}

async fn post_uat_check(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let scenario = body
        .get("scenario")
        .and_then(|v| v.as_str())
        .and_then(|raw| SubjectRef::parse(raw).ok())
        .ok_or_else(|| ApiError::bad_request("missing `scenario` identity string".to_owned()))?;
    let verdict = match body.get("verdict").and_then(|v| v.as_str()) {
        Some("pass") => UatVerdict::Pass,
        Some("fail") => UatVerdict::Fail,
        Some("blocked") => UatVerdict::Blocked,
        _ => {
            return Err(ApiError::bad_request(
                "`verdict` must be pass | fail | blocked".to_owned(),
            ))
        }
    };
    let evidence_ref = body
        .get("evidence_ref")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let note = body
        .get("note")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let actor = body
        .get("actor")
        .and_then(|v| v.as_str())
        .and_then(|raw| vistalith_domain::Actor::new(raw).ok())
        .unwrap_or_else(|| vistalith_domain::Actor::new("user:uat").expect("static actor"));

    let mut store = state.store.write().await;
    if store.graph().subject(&scenario).is_none() {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("unknown scenario `{scenario}`"),
        });
    }
    let check = SubjectRef::new(
        Namespace::Verification,
        SubjectKind::HumanCheck,
        uuid::Uuid::now_v7().to_string(),
    )
    .expect("generated check id is valid");
    store
        .append(vistalith_domain::VEvent {
            event_id: uuid::Uuid::now_v7(),
            actor,
            timestamp: time::OffsetDateTime::now_utc(),
            subjects: vec![check.clone(), scenario.clone()],
            correlation_id: uuid::Uuid::now_v7(),
            causation_id: None,
            trace_id: None,
            payload: vistalith_domain::EventPayload::UatCheckRecorded(UatCheckRecorded {
                check: check.clone(),
                scenario: scenario.clone(),
                verdict,
                evidence_ref,
                note,
            }),
        })
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    tracing::info!(scenario = %scenario, verdict = ?verdict, "UAT check recorded");
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "check": check.to_string(), "verdict": verdict })),
    ))
}

async fn get_uat_lens(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
    let mut scenarios: Vec<serde_json::Value> = Vec::new();
    for node in store.graph().subjects_of_kind(&SubjectKind::UatScenario) {
        let scenario = node.subject.to_string();
        let mut checks = Vec::new();
        let mut latest_verdict = String::from("unverified");
        for fact in store.graph().outgoing(&node.subject) {
            if fact.relation.kind != RelationKind::Contains {
                continue;
            }
            let Some(check) = store.graph().subject(&fact.relation.to) else {
                continue;
            };
            if check.subject.kind() != &SubjectKind::HumanCheck {
                continue;
            }
            let verdict = check
                .properties
                .get("verdict")
                .and_then(|v| v.as_str())
                .unwrap_or("unverified")
                .to_owned();
            latest_verdict = verdict.clone();
            checks.push(serde_json::json!({
                "check": check.subject.to_string(),
                "verdict": verdict,
                "note": check.properties.get("note").cloned(),
                "evidence_ref": check.properties.get("evidence_ref").cloned(),
            }));
        }
        scenarios.push(serde_json::json!({
            "scenario": scenario,
            "title": node.properties.get("title").cloned().unwrap_or(serde_json::json!("")),
            "status": node.properties.get("status").cloned().unwrap_or(serde_json::json!("unverified")),
            "latest_verdict": latest_verdict,
            "checks": checks,
        }));
    }
    Json(serde_json::json!({ "scenarios": scenarios }))
}

// --- Visual thinking canvas (slice 17, VISUAL-THINKING.md) -------------------

const CANVAS_KINDS: [(&str, SubjectKind); 4] = [
    ("note", SubjectKind::Note),
    ("question", SubjectKind::Question),
    ("hypothesis", SubjectKind::Hypothesis),
    ("option", SubjectKind::Option),
];

fn parse_canvas_kind(raw: &str) -> Option<SubjectKind> {
    CANVAS_KINDS
        .iter()
        .find(|(name, _)| *name == raw)
        .map(|(_, kind)| kind.clone())
}

/// POST /canvas/subjects — a free-form thinking primitive (note, question,
/// hypothesis, option) becomes a Vistalith-owned ADVISORY semantic subject:
/// sketching is a first-class engineering activity, and the primitive is
/// already semantic (VISUAL-THINKING.md progressive formalization step 1).
async fn post_canvas_subject(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let kind_name = body
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("missing `kind` (note|question|hypothesis|option)".to_owned()))?
        .to_owned();
    let Some(kind) = parse_canvas_kind(&kind_name) else {
        return Err(ApiError::bad_request(format!(
            "unknown canvas kind `{kind_name}` (note|question|hypothesis|option)"
        )));
    };
    let content = body
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("missing `content` string".to_owned()))?
        .to_owned();
    let actor = body
        .get("actor")
        .and_then(|v| v.as_str())
        .and_then(|raw| vistalith_domain::Actor::new(raw).ok())
        .unwrap_or_else(|| vistalith_domain::Actor::new("user:canvas").expect("static actor"));

    let subject = SubjectRef::new(
        Namespace::Vistalith,
        kind.clone(),
        uuid::Uuid::now_v7().to_string(),
    )
    .expect("generated canvas id is valid");
    let mut properties = std::collections::BTreeMap::from([
        ("content".to_owned(), serde_json::json!(content)),
        ("canvas_kind".to_owned(), serde_json::json!(kind_name)),
    ]);
    if let Some(link) = body
        .get("relates_to")
        .and_then(|v| v.as_str())
    {
        let target = SubjectRef::parse(link)
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        properties.insert(
            "relates_to".to_owned(),
            serde_json::json!(target.to_string()),
        );
    }

    let mut store = state.store.write().await;
    // Advisory relation when the primitive is attached to a semantic subject.
    let mut event_subjects = vec![subject.clone()];
    if let Some(link) = body.get("relates_to").and_then(|v| v.as_str()) {
        let target = SubjectRef::parse(link)
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        if store.graph().subject(&target).is_none() {
            return Err(ApiError {
                status: StatusCode::NOT_FOUND,
                message: format!("relates_to subject `{target}` does not exist"),
            });
        }
        event_subjects.push(target.clone());
    }
    store
        .append(vistalith_domain::VEvent {
            event_id: uuid::Uuid::now_v7(),
            actor: actor.clone(),
            timestamp: time::OffsetDateTime::now_utc(),
            subjects: event_subjects.clone(),
            correlation_id: uuid::Uuid::now_v7(),
            causation_id: None,
            trace_id: None,
            payload: vistalith_domain::EventPayload::SubjectDefined(
                vistalith_domain::SubjectDefined {
                    subject: subject.clone(),
                    authority: vistalith_domain::AuthorityClass::Advisory,
                    provenance: vistalith_domain::Provenance::new(actor.as_str())
                        .expect("validated actor"),
                    properties,
                },
            ),
        })
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    // Advisory mention edge to the linked subject (step 2: semantic
    // annotation).
    if let Some(link) = body.get("relates_to").and_then(|v| v.as_str()) {
        let target = SubjectRef::parse(link).expect("validated above");
        store
            .append(vistalith_domain::VEvent {
                event_id: uuid::Uuid::now_v7(),
                actor,
                timestamp: time::OffsetDateTime::now_utc(),
                subjects: vec![subject.clone(), target.clone()],
                correlation_id: uuid::Uuid::now_v7(),
                causation_id: None,
                trace_id: None,
                payload: vistalith_domain::EventPayload::RelationDeclared(
                    vistalith_domain::RelationDeclared {
                        fact: vistalith_domain::RelationFact {
                            relation: vistalith_domain::RelationRef::new(
                                subject.clone(),
                                vistalith_domain::RelationKind::Mentions,
                                target.clone(),
                            )
                            .map_err(|e| ApiError::bad_request(e.to_string()))?,
                            authority: vistalith_domain::AuthorityClass::Advisory,
                            provenance: vistalith_domain::Provenance::new("user:canvas")
                                .expect("static provenance"),
                        },
                    },
                ),
            })
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
    }
    tracing::info!(subject = %subject, kind = %kind_name, "canvas primitive created");
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "subject": subject.to_string(), "kind": kind_name })),
    ))
}

async fn get_canvas_subjects(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
    let canvas_kinds = CANVAS_KINDS
        .iter()
        .map(|(_, kind)| kind.clone())
        .collect::<Vec<_>>();
    let mut subjects: Vec<serde_json::Value> = store
        .graph()
        .subjects()
        .filter(|node| canvas_kinds.contains(node.subject.kind()))
        .map(|node| {
            serde_json::json!({
                "subject": node.subject.to_string(),
                "kind": node.properties.get("canvas_kind").cloned().unwrap_or(
                    serde_json::json!(node.subject.kind().as_str()),
                ),
                "content": node.properties.get("content").cloned().unwrap_or(serde_json::Value::Null),
                "relates_to": node.properties.get("relates_to").cloned(),
                "authority": node.authority,
                "deprecated": node.deprecated,
            })
        })
        .collect();
    subjects.sort_by(|a, b| a["subject"].to_string().cmp(&b["subject"].to_string()));
    Json(serde_json::json!({ "subjects": subjects }))
}

/// POST /canvas/subjects/{ns}/{kind}/{id}/promote — progressive
/// formalization (VISUAL-THINKING.md): the sketch primitive becomes a
/// candidate VisualIntent draft (SPEC-006). Still drafts only: promotion
/// to the graph remains the explicit SPEC-006 act.
async fn post_canvas_promote(
    State(state): State<AppState>,
    Path((ns, kind, id)): Path<(String, String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let primitive = SubjectRef::new(
        Namespace::parse(&ns).map_err(|e| ApiError::bad_request(e.to_string()))?,
        SubjectKind::parse(&kind).map_err(|e| ApiError::bad_request(e.to_string()))?,
        id,
    )
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let actor = body
        .get("actor")
        .and_then(|v| v.as_str())
        .and_then(|raw| vistalith_domain::Actor::new(raw).ok())
        .unwrap_or_else(|| vistalith_domain::Actor::new("user:canvas").expect("static actor"));
    let gesture = body
        .get("gesture")
        .and_then(|v| v.as_str())
        .unwrap_or("annotate")
        .to_owned();

    let mut store = state.store.write().await;
    let node = store.graph().subject(&primitive).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("unknown canvas subject `{primitive}`"),
    })?;
    let content = node
        .properties
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let target = node
        .properties
        .get("relates_to")
        .and_then(|v| v.as_str())
        .and_then(|raw| SubjectRef::parse(raw).ok());

    let Some(target) = target else {
        return Err(ApiError::bad_request(
            "canvas primitive has no `relates_to` subject to formalize against;              attach it to a semantic subject first"
                .to_owned(),
        ));
    };

    let intent = draft_intent(
        &mut store,
        &target,
        gesture,
        serde_json::json!({
            "operations": [],
            "from_canvas": primitive.to_string(),
            "content": content,
        }),
        Some(format!("promoted from canvas primitive {primitive}")),
        &actor,
    )?;
    tracing::info!(canvas = %primitive, intent = %intent, "canvas primitive formalized");
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "intent": intent.to_string(),
            "target": target.to_string(),
        })),
    ))
}

async fn get_decisions_lens(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.store.read().await;
    let lens: DecisionsLens = vistalith_graph::decisions_lens(store.graph());
    Json(serde_json::to_value(lens).expect("lens serialization"))
}

async fn post_sddk_sync(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(bridge) = state.sddk_bridge.as_ref() else {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: "no SDDK bridge configured (start with --sddk-ledger/--sddk-workflow)".to_owned(),
        });
    };
    let actor = vistalith_domain::Actor::new("system:sddk-sync")
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let mut store = state.store.write().await;
    let report = bridge
        .sync_workflow(&mut store, &actor)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    tracing::info!(created = report.subjects_created, updated = report.subjects_updated, "SDDK workflow synced");
    Ok(Json(serde_json::to_value(report).expect("report serialization")))
}

async fn get_why(
    State(state): State<AppState>,
    Path((namespace, kind, id)): Path<(String, String, String)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let subject = SubjectRef::new(
        Namespace::parse(&namespace).map_err(|e| ApiError::bad_request(e.to_string()))?,
        SubjectKind::parse(&kind).map_err(|e| ApiError::bad_request(e.to_string()))?,
        id,
    )
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let max_depth = match params.get("depth") {
        None => 3,
        Some(raw) => raw
            .parse::<u8>()
            .map_err(|e| ApiError::bad_request(format!("invalid `depth`: {e}")))?,
    };
    let store = state.store.read().await;
    let path = why_path(store.graph(), &subject, max_depth).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("unknown subject `{subject}`"),
    })?;
    Ok(Json(serde_json::to_value(path).expect("why serialization")))
}

async fn get_mcp_server_health(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let connection = state.mcp.get(&name).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("unknown MCP server `{name}`"),
    })?;
    let status = connection.status();
    Ok(Json(serde_json::to_value(status).expect("status serialization")))
}

async fn post_mcp_server_refresh(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let connection = state.mcp.get(&name).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("unknown MCP server `{name}`"),
    })?;
    let tools = connection
        .refresh()
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    tracing::info!(server = %name, tools, "MCP tools re-discovered");
    Ok(Json(serde_json::json!({ "server": name, "tools": tools })))
}

async fn post_mcp_server_disable(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !state.mcp.set_disabled(&name, true) {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("unknown MCP server `{name}`"),
        });
    }
    Ok(Json(serde_json::json!({ "server": name, "disabled": true })))
}

async fn post_mcp_server_enable(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !state.mcp.set_disabled(&name, false) {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: format!("unknown MCP server `{name}`"),
        });
    }
    Ok(Json(serde_json::json!({ "server": name, "disabled": false })))
}

/// POST /sddk/pull-up — evaluate a Vistalith innovation against the SDDK
/// focus test and, when it classifies as SDDK_PROPOSAL (or the caller
/// declares a spike), submit it as governed evidence through the SDDK
/// capability gateway (milestone M10).
#[allow(clippy::too_many_lines)]
async fn post_sddk_pull_up(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let Some(bridge) = state.sddk_bridge.as_ref() else {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: "no SDDK bridge configured (start with --sddk-ledger/--sddk-workflow)".to_owned(),
        });
    };
    let feature = body
        .get("feature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("missing `feature` string".to_owned()))?
        .to_owned();
    let semantic_core = body
        .get("semantic_core")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let parse_answer = |key: &str| -> Result<FocusAnswer, ApiError> {
        match body.get(key).and_then(|v| v.as_str()) {
            Some("yes") => Ok(FocusAnswer::Yes),
            Some("no") => Ok(FocusAnswer::No),
            _ => Err(ApiError::bad_request(format!(
                "`{key}` must be \"yes\" or \"no\""
            ))),
        }
    };
    let focus_test = FocusTest {
        gui_free: parse_answer("gui_free")?,
        llm_free: parse_answer("llm_free")?,
        semantic_relevance: parse_answer("semantic_relevance")?,
        no_duplicated_authority: parse_answer("no_duplicated_authority")?,
        deterministic: parse_answer("deterministic")?,
    };
    let mut evidence = Vec::new();
    if let Some(list) = body.get("evidence").and_then(|v| v.as_array()) {
        for raw in list {
            evidence.push(
                raw.as_str()
                    .ok_or_else(|| {
                        ApiError::bad_request("`evidence` entries must be strings".to_owned())
                    })?
                    .to_owned(),
            );
        }
    }
    let proposed_horizon = body
        .get("proposed_horizon")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let evaluation = PullUpEvaluation {
        feature: feature.clone(),
        semantic_core,
        focus_test,
        evidence,
        proposed_horizon,
    };
    let intent = body
        .get("intent")
        .and_then(|v| v.as_str())
        .and_then(|raw| SubjectRef::parse(raw).ok());
    let target = body
        .get("target")
        .and_then(|v| v.as_str())
        .and_then(|raw| SubjectRef::parse(raw).ok());
    let actor = body
        .get("actor")
        .and_then(|v| v.as_str())
        .and_then(|raw| vistalith_domain::Actor::new(raw).ok())
        .unwrap_or_else(|| {
            vistalith_domain::Actor::new("system:pull-up")
                .expect("static actor")
        });

    let mut store = state.store.write().await;
    // The evaluation target defaults to the observed SDDK project subject.
    let target = target.unwrap_or_else(|| {
        SubjectRef::new(
            Namespace::Sddk,
            SubjectKind::Project,
            bridge.project_id().to_owned(),
        )
        .expect("project id is a valid subject id")
    });
    // The intent subject is optional: without one, the evaluation submits
    // against the project directly (the bridge requires an existing intent,
    // so synthesize one when absent).
    // The synthesized intent drafts against `target`; the projection
    // requires it to exist, so materialize the observed project subject
    // (derived) when absent.
    if store.graph().subject(&target).is_none() {
        store
            .append(vistalith_domain::VEvent {
                event_id: uuid::Uuid::now_v7(),
                actor: actor.clone(),
                timestamp: time::OffsetDateTime::now_utc(),
                subjects: vec![target.clone()],
                correlation_id: uuid::Uuid::now_v7(),
                causation_id: None,
                trace_id: None,
                payload: vistalith_domain::EventPayload::SubjectDefined(
                    vistalith_domain::SubjectDefined {
                        subject: target.clone(),
                        authority: vistalith_domain::AuthorityClass::Derived,
                        provenance: vistalith_domain::Provenance::new("system:pull-up")
                            .expect("static provenance"),
                        properties: std::collections::BTreeMap::from([(
                            "project_id".to_owned(),
                            serde_json::json!(bridge.project_id()),
                        )]),
                    },
                ),
            })
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
    }

    let synthesized;
    let intent = match intent {
        Some(intent) => intent,
        None => {
            synthesized = SubjectRef::new(
                Namespace::Visual,
                SubjectKind::VisualProposal,
                uuid::Uuid::now_v7().to_string(),
            )
            .expect("generated intent id is valid");
            let base_revision = store.graph().revision();
            store
                .append(vistalith_domain::VEvent {
                    event_id: uuid::Uuid::now_v7(),
                    actor: actor.clone(),
                    timestamp: time::OffsetDateTime::now_utc(),
                    subjects: vec![synthesized.clone(), target.clone()],
                    correlation_id: uuid::Uuid::now_v7(),
                    causation_id: None,
                    trace_id: None,
                    payload: vistalith_domain::EventPayload::IntentDrafted(
                        vistalith_domain::IntentDrafted {
                            intent: synthesized.clone(),
                            target: target.clone(),
                            gesture: "pull-up".to_owned(),
                            change: serde_json::json!({ "operations": [] }),
                            base_revision: base_revision + 1,
                            reason: Some(format!("pull-up evaluation: {feature}")),
                        },
                    ),
                })
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            synthesized
        }
    };

    let approve = body.get("approve").and_then(|v| v.as_bool()).unwrap_or(false);
    let outcome = bridge
        .evaluate_pull_up(&mut store, &intent, &target, &evaluation, &actor, approve)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    tracing::info!(
        feature = %feature,
        classification = ?outcome.classification,
        receipt = ?outcome.receipt_id,
        "pull-up evaluated"
    );
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(&outcome).expect("outcome serialization")),
    ))
}

async fn get_sddk_receipts(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(bridge) = state.sddk_bridge.as_ref() else {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: "no SDDK bridge configured (start with --sddk-ledger/--sddk-workflow)".to_owned(),
        });
    };
    let receipts = bridge
        .receipts()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(serde_json::json!({ "receipts": receipts })))
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
    IntentError, Promotion, discard_intent as discard_intent_op,
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
    let approve = body.get("approve").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut store = state.store.write().await;
    let outcome = match state.sddk_bridge.as_ref() {
        Some(bridge) => promote_intent_with_bridge(
            &mut store,
            &intent,
            &actor,
            Some(bridge),
            approve,
        )?,
        None => promote_intent_op(&mut store, &intent, &actor)?,
    };
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
        Promotion::SubmittedToSddk {
            subject,
            proposal,
            receipt_id,
            decision,
        } => (
            StatusCode::OK,
            Json(serde_json::json!({
                "outcome": "submitted-to-sddk",
                "subject": subject.to_string(),
                "proposal": proposal.to_string(),
                "decision": decision,
                "receipt_id": receipt_id,
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
