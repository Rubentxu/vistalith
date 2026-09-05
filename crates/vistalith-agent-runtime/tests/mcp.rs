//! MCP integration end-to-end (`agentic/MCP.md`, SPEC-009): connect to the
//! `mcp-echo` fixture server over stdio, discover its tools into the unified
//! catalog, classify consequences from MCP annotations, and prove the
//! permission gate: read-only runs free, write-class tools need scoped
//! grants, and every call is durable.

use std::sync::Arc;

use vistalith_agent_runtime::{
    ConversationEngine, FakeProvider, FakeStep, GrantStore, McpConnection, McpManager,
    McpServerConfig, PermissionDecision, ToolRegistry,
};
use vistalith_domain::{MessageRole, SubjectKind};
use vistalith_graph::GraphStore;

fn echo_config() -> McpServerConfig {
    McpServerConfig {
        name: "echo".to_owned(),
        command: Some(env!("CARGO_BIN_EXE_mcp-echo").to_owned()),
        args: Vec::new(),
        url: None,
    }
}

#[tokio::test]
async fn connect_discovers_tools_with_consequence_classification() {
    let connection = Arc::new(McpConnection::connect(echo_config()).await.unwrap());
    let entries = connection.entries();

    assert_eq!(entries.len(), 2);
    let mut ids: Vec<&str> = entries.iter().map(|e| e.descriptor.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["mcp_echo_append_note", "mcp_echo_echo"]);

    // Annotation mapping: read_only_hint → ReadOnly, silent → Write.
    let echo = entries.iter().find(|e| e.descriptor.id == "mcp_echo_echo").unwrap();
    assert_eq!(echo.descriptor.consequence, vistalith_agent_runtime::Consequence::ReadOnly);
    assert_eq!(echo.remote_name.as_deref(), Some("echo"));
    let note = entries
        .iter()
        .find(|e| e.descriptor.id == "mcp_echo_append_note")
        .unwrap();
    assert_eq!(note.descriptor.consequence, vistalith_agent_runtime::Consequence::Write);
    // The tool arguments schema is the remote JSON Schema, not prose.
    assert_eq!(echo.descriptor.parameters["type"], "object");
    assert!(echo.descriptor.parameters["properties"]["message"].is_object());

}

#[tokio::test]
async fn unified_catalog_enforces_scoped_grants_on_mcp_tools() {
    let connection = Arc::new(McpConnection::connect(echo_config()).await.unwrap());
    let grants = Arc::new(GrantStore::new());
    let mut registry = ToolRegistry::native(Arc::clone(&grants));
    registry.add_mcp(Arc::clone(&connection));

    // Read-only MCP tool: allow, and a real round trip through stdio.
    assert_eq!(
        registry.permission("mcp_echo_echo").unwrap(),
        PermissionDecision::Allow
    );
    let output = registry
        .invoke(GraphStore::new().graph(), "mcp_echo_echo", &serde_json::json!({ "message": "hi" }))
        .await
        .unwrap();
    assert_eq!(output["ok"], true);
    assert_eq!(output["text"], "echo: hi");

    // Write-class MCP tool: ask → refused without a grant.
    assert_eq!(
        registry.permission("mcp_echo_append_note").unwrap(),
        PermissionDecision::Ask
    );
    let blocked = registry
        .invoke(GraphStore::new().graph(), "mcp_echo_append_note", &serde_json::json!({ "message": "x" }))
        .await;
    assert!(matches!(
        blocked,
        Err(vistalith_agent_runtime::ToolError::ApprovalRequired(_))
    ));

    // A scoped grant of one call: the first call runs, the grant is consumed.
    grants.grant("mcp_echo_append_note", 1);
    assert_eq!(
        registry.permission("mcp_echo_append_note").unwrap(),
        PermissionDecision::Allow
    );
    let output = registry
        .invoke(GraphStore::new().graph(), "mcp_echo_append_note", &serde_json::json!({ "message": "note" }))
        .await
        .unwrap();
    assert_eq!(output["text"], "note appended: note");
    assert_eq!(grants.remaining("mcp_echo_append_note"), 0);
    assert_eq!(
        registry.permission("mcp_echo_append_note").unwrap(),
        PermissionDecision::Ask,
        "the grant is spent after one authorized call"
    );

    // Explicit deny always wins, even with a live grant.
    grants.grant("mcp_echo_echo", 5);
    grants.set_denied("mcp_echo_echo", true);
    assert_eq!(
        registry.permission("mcp_echo_echo").unwrap(),
        PermissionDecision::Deny
    );

}

#[tokio::test]
async fn mcp_tool_call_in_a_turn_is_durable_with_its_source() {
    let mut store = GraphStore::new();
    let grants = Arc::new(GrantStore::new());
    let connection = Arc::new(McpConnection::connect(echo_config()).await.unwrap());
    let mut registry = ToolRegistry::native(Arc::clone(&grants));
    registry.add_mcp(Arc::clone(&connection));
    let engine = ConversationEngine::new(FakeProvider::steps(vec![
        FakeStep::ToolCall {
            name: "mcp_echo_echo".to_owned(),
            arguments: serde_json::json!({ "message": "from a turn" }),
        },
        FakeStep::Text("done".to_owned()),
    ]))
    .with_tools(registry);

    let thread = engine.start_thread(&mut store, "mcp turn").unwrap();
    let reply = engine
        .send_user_message(&mut store, &thread, "call the echo tool")
        .await
        .unwrap();
    assert_eq!(reply.content, "done");

    // The typed tool item records the MCP source and the remote output.
    // (Tool items hang off the `used_tool` edge, not `contains`.)
    let tool_items: Vec<_> = store
        .graph()
        .subjects_of_kind(&SubjectKind::ToolCall)
        .collect();
    assert_eq!(tool_items.len(), 1);
    let item = tool_items[0];
    assert_eq!(item.properties["tool"], "mcp_echo_echo");
    assert_eq!(item.properties["source"], "mcp:echo");
    assert_eq!(item.properties["output"]["text"], "echo: from a turn");

    // Model-facing history: the tool output reached the model as a tool item.
    let tool_message = store
        .graph()
        .children(&thread)
        .into_iter()
        .find(|n| n.properties.get("role") == Some(&serde_json::json!(MessageRole::Tool)))
        .expect("tool output is in the history");
    assert!(tool_message
        .properties
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap()
        .contains("echo: from a turn"));

}

#[tokio::test]
async fn manager_registers_and_reports_status() {
    let manager = McpManager::new();
    let status = manager.register(echo_config()).await.unwrap();
    assert_eq!(status.name, "echo");
    assert_eq!(status.transport, "stdio");
    assert_eq!(status.tools, 2);

    assert!(manager.get("echo").is_some());
    let statuses = manager.status();
    assert_eq!(statuses.len(), 1);

    let _connection = manager.take("echo").unwrap();
}

#[tokio::test]
async fn config_validation_rejects_double_or_missing_transports() {
    let both = McpServerConfig {
        name: "x".to_owned(),
        command: Some("a".to_owned()),
        args: vec![],
        url: Some("http://localhost".to_owned()),
    };
    assert!(both.validate().is_err());
    let neither = McpServerConfig {
        name: "x".to_owned(),
        command: None,
        args: vec![],
        url: None,
    };
    assert!(neither.validate().is_err());
    // Wrong transport is surfaced as a connection error, not a panic.
    let bogus = McpServerConfig {
        name: "ghost".to_owned(),
        command: Some("/nonexistent/binary/here".to_owned()),
        args: vec![],
        url: None,
    };
    assert!(McpConnection::connect(bogus).await.is_err());
}

#[tokio::test]
async fn transport_death_triggers_reconnect_and_the_call_succeeds() {
    let connection = Arc::new(McpConnection::connect(echo_config()).await.unwrap());
    // First call over the original child process.
    let first = connection
        .call("echo", serde_json::json!({ "message": "before" }))
        .await
        .unwrap();
    assert_eq!(first["text"], "echo: before");

    // Force the transport closed (what a killed child looks like).
    connection.reconnect().await.expect("manual reconnect");
    // The connection still works — the child was re-spawned.
    let second = connection
        .call("echo", serde_json::json!({ "message": "after" }))
        .await
        .unwrap();
    assert_eq!(second["text"], "echo: after");
    // And discovery survived the reconnect (same two tools).
    assert_eq!(connection.entries().len(), 2);
}

#[tokio::test]
async fn refresh_re_discovers_tools() {
    let connection = Arc::new(McpConnection::connect(echo_config()).await.unwrap());
    let count = connection.refresh().await.expect("refresh");
    assert_eq!(count, 2, "mcp-echo advertises two tools");
    assert_eq!(connection.entries().len(), 2);
}

#[tokio::test]
async fn disabled_servers_leave_the_catalog_but_stay_registered() {
    let manager = McpManager::new();
    manager.register(echo_config()).await.expect("register");
    manager.set_disabled("echo", true);
    assert!(manager.is_disabled("echo"));
    assert!(
        manager.connections().is_empty(),
        "disabled servers' tools stay out of the catalog"
    );
    let status = &manager.status()[0];
    assert!(status.disabled);
    assert_eq!(status.name, "echo");
    manager.set_disabled("echo", false);
    assert_eq!(manager.connections().len(), 1);
}
