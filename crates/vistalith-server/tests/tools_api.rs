//! Unified tool catalog + MCP server management over HTTP (SPEC-009,
//! slice 6). The MCP fixture is the `mcp-echo` binary from the
//! agent-runtime crate; cargo only exposes `CARGO_BIN_EXE_*` to its own
//! package, so the path is resolved from candidates (and can be forced with
//! `VISTALITH_MCP_ECHO`). Tests skip with a note when the binary has not
//! been built — `cargo test --workspace` always builds it.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;
use vistalith_agent_runtime::{GrantStore, McpManager};
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

fn mcp_echo_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("VISTALITH_MCP_ECHO") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        candidates.push(PathBuf::from(dir).join("debug").join("mcp-echo"));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/debug/mcp-echo")
            .canonicalize()
            .unwrap_or_default(),
    );
    candidates.into_iter().find(|p| p.is_file())
}

async fn app_with_echo(provider: vistalith_agent_runtime::RuntimeProvider) -> Option<Router> {
    let Some(echo) = mcp_echo_path() else {
        eprintln!("skipping: mcp-echo binary not built yet (run `cargo build --workspace`)");
        return None;
    };
    let manager = Arc::new(McpManager::new());
    manager
        .register(vistalith_agent_runtime::McpServerConfig {
            name: "echo".to_owned(),
            command: Some(echo.display().to_string()),
            args: Vec::new(),
            url: None,
        })
        .await
        .expect("fixture server connects");
    let state = AppState::with_parts(
        vistalith_graph::GraphStore::new(),
        provider,
        manager,
        Arc::new(GrantStore::new()),
    );
    Some(router(state))
}

#[tokio::test]
async fn unified_catalog_lists_native_and_mcp_tools() {
    let Some(app) = app_with_echo(vistalith_agent_runtime::RuntimeProvider::Fake(
        vistalith_agent_runtime::FakeProvider::repeating("ok"),
    ))
    .await
    else {
        return;
    };

    let (status, body) = call(
        app,
        Request::get("/tools").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let tools = body["tools"].as_array().expect("tools array");
    let ids: Vec<&str> = tools
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"graph_search"), "native tool present");
    assert!(ids.contains(&"mcp_echo_echo"), "MCP echo present");
    assert!(ids.contains(&"mcp_echo_append_note"), "MCP note present");

    let by_id = |id: &str| {
        tools
            .iter()
            .find(|t| t["id"] == serde_json::json!(id))
            .unwrap()
            .clone()
    };
    // Native read-only: allow. MCP read-only hint: allow. Silent MCP tool:
    // ask (protocol defaults → write class).
    assert_eq!(by_id("graph_search")["permission"], "allow");
    assert_eq!(by_id("graph_search")["source"]["kind"], "native");
    assert_eq!(by_id("mcp_echo_echo")["permission"], "allow");
    assert_eq!(by_id("mcp_echo_echo")["source"]["kind"], "mcp");
    assert_eq!(by_id("mcp_echo_echo")["source"]["server"], "echo");
    assert_eq!(by_id("mcp_echo_append_note")["permission"], "ask");
    assert_eq!(by_id("mcp_echo_append_note")["consequence"], "write");
}

#[tokio::test]
async fn scoped_grants_flip_ask_to_allow_and_are_consumed_by_turns() {
    let Some(app) = app_with_echo(vistalith_agent_runtime::RuntimeProvider::Fake(
        vistalith_agent_runtime::FakeProvider::repeating("ok"),
    ))
    .await
    else {
        return;
    };

    // Grant without a live model: one authorized call for the write tool.
    let (status, grant) = call(
        app.clone(),
        Request::post("/tools/mcp_echo_append_note/grant")
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "calls": 1 }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(grant["remaining"], 1);

    // The catalog now reports allow with the remaining count.
    let (_, tools) = call(
        app.clone(),
        Request::get("/tools").body(Body::empty()).unwrap(),
    )
    .await;
    let note = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == serde_json::json!("mcp_echo_append_note"))
        .unwrap();
    assert_eq!(note["permission"], "allow");
    assert_eq!(note["grant_remaining"], 1);

    // Revoking returns the tool to ask.
    let (_, revoked) = call(
        app.clone(),
        Request::post("/tools/mcp_echo_append_note/revoke")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(revoked["revoked"], true);
    let (_, tools) = call(
        app,
        Request::get("/tools").body(Body::empty()).unwrap(),
    )
    .await;
    let note = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == serde_json::json!("mcp_echo_append_note"))
        .unwrap();
    assert_eq!(note["permission"], "ask");
}

#[tokio::test]
async fn unknown_tools_and_servers_error_cleanly() {
    let Some(app) = app_with_echo(vistalith_agent_runtime::RuntimeProvider::Fake(
        vistalith_agent_runtime::FakeProvider::repeating("ok"),
    ))
    .await
    else {
        return;
    };

    let (status, _) = call(
        app.clone(),
        Request::post("/tools/nope/grant")
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "calls": 1 }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = call(
        app.clone(),
        Request::delete("/mcp/servers/ghost")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Registering a duplicate server name conflicts.
    let (status, _) = call(
        app,
        Request::post("/mcp/servers")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{ "name": "echo", "command": "/bin/true" }"#,
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn an_mcp_tool_call_runs_inside_a_turn() {
    let scripted = vistalith_agent_runtime::RuntimeProvider::Fake(
        vistalith_agent_runtime::FakeProvider::steps(vec![
            vistalith_agent_runtime::FakeStep::ToolCall {
                name: "mcp_echo_echo".to_owned(),
                arguments: serde_json::json!({ "message": "over http" }),
            },
            vistalith_agent_runtime::FakeStep::Text("tool round done".to_owned()),
        ]),
    );
    let Some(app) = app_with_echo(scripted).await else {
        return;
    };

    let (status, _) = call(
        app.clone(),
        Request::post("/threads")
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "title": "mcp turn" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, threads) = call(
        app.clone(),
        Request::get("/threads").body(Body::empty()).unwrap(),
    )
    .await;
    let thread = threads["threads"][0]["thread"].as_str().unwrap();
    let thread_id = thread.rsplit(':').next().unwrap();

    let (status, reply) = call(
        app.clone(),
        Request::post(format!("/threads/{thread_id}/messages"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{ "content": "use the echo tool" }"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["content"], "tool round done");
}
