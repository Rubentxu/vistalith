//! MCP client integration (`agentic/MCP.md`, SPEC-009, ADR-009).
//!
//! Vistalith is an MCP *client*: it connects to external tool servers over
//! stdio (child process) or Streamable HTTP through the official Rust SDK
//! (`rmcp`, direct — no façade), discovers their tools once at connect time,
//! and projects them into the unified tool catalog (`crate::tools`) where
//! Vistalith permissions apply to every call. Credentials never cross the
//! client boundary into any renderer (SPEC-008).
//!
//! Deliberately out of scope for this slice (SPK-007 continues later):
//! authenticated remote servers, `tools/list_changed` re-discovery and
//! automatic reconnect — the manager records connection health and a failed
//! call surfaces as a tool error; reconnecting re-discovers.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rmcp::model::{CallToolRequestParams, ClientInfo, ContentBlock, ToolAnnotations};
use rmcp::service::{RoleClient, RunningService, serve_client};
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::tools::{Consequence, ToolDescriptor, ToolEntry, ToolError, ToolSource};

#[derive(Debug, Error)]
pub enum McpError {
    #[error("MCP server `{0}`: {1}")]
    Connect(String, String),
    #[error("MCP tool discovery failed on `{0}`: {1}")]
    Discovery(String, String),
    #[error("invalid MCP server config: {0}")]
    InvalidConfig(String),
}

/// How to reach one MCP server. Exactly one transport is set:
/// stdio (child process command) or Streamable HTTP (`url`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Local, unique server name; tool ids are namespaced under it.
    pub name: String,
    /// stdio transport: executable to spawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Streamable HTTP transport: base URL of the MCP endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl McpServerConfig {
    pub fn validate(&self) -> Result<(), McpError> {
        if self.name.trim().is_empty() {
            return Err(McpError::InvalidConfig("missing `name`".to_owned()));
        }
        match (&self.command, &self.url) {
            (Some(_), None) | (None, Some(_)) => Ok(()),
            (Some(_), Some(_)) => Err(McpError::InvalidConfig(format!(
                "server `{}` sets both `command` and `url`",
                self.name
            ))),
            (None, None) => Err(McpError::InvalidConfig(format!(
                "server `{}` needs `command` (stdio) or `url` (http)",
                self.name
            ))),
        }
    }
}

/// Connection status as reported by the API (health from `agentic/MCP.md`'s
/// server model; reconnect handling lands with SPK-007 follow-ups).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionStatus {
    Connected,
    Unhealthy,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpServerStatus {
    pub name: String,
    pub transport: String,
    pub status: ConnectionStatus,
    pub tools: usize,
}

/// One live MCP client connection plus the tool descriptors discovered at
/// connect time. `RunningService` is internally concurrent, so a connection
/// is shared through an `Arc` across requests.
pub struct McpConnection {
    config: McpServerConfig,
    service: RunningService<RoleClient, ClientInfo>,
    descriptors: Vec<ToolDescriptor>,
}

impl McpConnection {
    /// Connects, negotiates the session and discovers tools.
    pub async fn connect(config: McpServerConfig) -> Result<Self, McpError> {
        config.validate()?;
        let service: RunningService<RoleClient, ClientInfo> = match (&config.command, &config.url)
        {
            (Some(command), None) => {
                let mut cmd = tokio::process::Command::new(command);
                cmd.args(&config.args);
                let transport = TokioChildProcess::new(cmd)
                    .map_err(|e| McpError::Connect(config.name.clone(), e.to_string()))?;
                serve_client(ClientInfo::default(), transport)
                    .await
                    .map_err(|e| McpError::Connect(config.name.clone(), e.to_string()))?
            }
            (None, Some(url)) => {
                let transport = StreamableHttpClientTransport::from_uri(url.as_str());
                serve_client(ClientInfo::default(), transport)
                    .await
                    .map_err(|e| McpError::Connect(config.name.clone(), e.to_string()))?
            }
            _ => return Err(McpError::InvalidConfig(config.name.clone())),
        };
        let listed = service
            .peer()
            .list_tools(None)
            .await
            .map_err(|e| McpError::Discovery(config.name.clone(), e.to_string()))?;

        let descriptors = listed
            .tools
            .iter()
            .map(|tool| mcp_tool_descriptor(&config.name, tool))
            .collect();
        Ok(McpConnection {
            config,
            service,
            descriptors,
        })
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    pub fn config(&self) -> &McpServerConfig {
        &self.config
    }

    pub fn status(&self) -> McpServerStatus {
        McpServerStatus {
            name: self.config.name.clone(),
            transport: if self.config.command.is_some() {
                "stdio".to_owned()
            } else {
                "http".to_owned()
            },
            status: if self.service.is_transport_closed() {
                ConnectionStatus::Unhealthy
            } else {
                ConnectionStatus::Connected
            },
            tools: self.descriptors.len(),
        }
    }

    /// Catalog entries for this connection (ids namespaced by server name).
    /// Takes `&Arc<Self>` so every entry can clone the shared handle the
    /// manager holds.
    pub fn entries(self: &Arc<Self>) -> Vec<ToolEntry> {
        self.descriptors
            .iter()
            .map(|descriptor| {
                let remote = remote_name_of(&descriptor.id, &self.config.name);
                ToolEntry::mcp(descriptor.clone(), remote, Arc::clone(self))
            })
            .collect()
    }

    /// Calls a remote tool by its *original* name.
    pub async fn call(&self, tool: &str, args: Value) -> Result<Value, ToolError> {
        // `CallToolRequestParams` is `#[non_exhaustive]`: mutate the default.
        let mut params = CallToolRequestParams::default();
        params.name = tool.to_owned().into();
        params.arguments = args.as_object().cloned();
        let result = self
            .service
            .peer()
            .call_tool(params)
            .await
            .map_err(|e| ToolError::Failed(tool.to_owned(), e.to_string()))?;
        let text: Vec<String> = result
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        Ok(serde_json::json!({
            "ok": !result.is_error.unwrap_or(false),
            "text": text.join("\n"),
            "structured": result.structured_content,
        }))
    }

    /// Cancels the session (and with stdio, terminates the child process).
    /// `cancel` consumes the `RunningService`; the last `Arc` owner does the
    /// actual shutdown, earlier callers only request it.
    pub async fn shutdown(self: Arc<Self>) {
        if let Ok(connection) = Arc::try_unwrap(self) {
            let _ = connection.service.cancel().await;
        }
    }
}

/// Manager for the live MCP connections (the `enabled/disabled` + `health`
/// rows of `agentic/MCP.md`'s server model).
#[derive(Default)]
pub struct McpManager {
    connections: Mutex<BTreeMap<String, Arc<McpConnection>>>,
}

impl McpManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Connects and registers a server. Failing to connect leaves the
    /// manager untouched and reports the error to the caller.
    pub async fn register(&self, config: McpServerConfig) -> Result<McpServerStatus, McpError> {
        config.validate()?;
        let connection = Arc::new(McpConnection::connect(config.clone()).await?);
        let status = connection.status();
        self.connections
            .lock()
            .expect("mcp manager lock")
            .insert(config.name.clone(), connection);
        Ok(status)
    }

    /// Removes a server, returning the connection for an orderly shutdown.
    pub fn take(&self, name: &str) -> Option<Arc<McpConnection>> {
        self.connections.lock().expect("mcp manager lock").remove(name)
    }

    pub fn get(&self, name: &str) -> Option<Arc<McpConnection>> {
        self.connections.lock().expect("mcp manager lock").get(name).cloned()
    }

    /// Snapshots the live connections (for building the unified catalog).
    pub fn connections(&self) -> Vec<Arc<McpConnection>> {
        self.connections
            .lock()
            .expect("mcp manager lock")
            .values()
            .cloned()
            .collect()
    }

    pub fn status(&self) -> Vec<McpServerStatus> {
        self.connections
            .lock()
            .expect("mcp manager lock")
            .values()
            .map(|c| c.status())
            .collect()
    }
}

/// Maps an MCP tool annotation set onto a Vistalith consequence class.
/// MCP annotations are *hints* with protocol defaults (read-only: false,
/// destructive: true), so servers that stay silent get the conservative
/// `Write` — an MCP tool can never sneak in as read-only.
pub fn consequence_from_annotations(annotations: Option<&ToolAnnotations>) -> Consequence {
    match annotations.and_then(|a| a.read_only_hint) {
        Some(true) => Consequence::ReadOnly,
        _ => match annotations.and_then(|a| a.destructive_hint) {
            Some(true) => Consequence::Destructive,
            _ => Consequence::Write,
        },
    }
}

fn mcp_tool_descriptor(server: &str, tool: &rmcp::model::Tool) -> ToolDescriptor {
    ToolDescriptor {
        id: catalog_id(server, &tool.name),
        description: tool
            .description
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default(),
        consequence: consequence_from_annotations(tool.annotations.as_ref()),
        parameters: serde_json::Value::Object((*tool.input_schema).clone()),
        source: ToolSource::Mcp {
            server: server.to_owned(),
        },
    }
}

/// Catalog id for an MCP tool: `mcp_<server>_<tool>`, sanitized to the
/// identifier shapes providers accept (`[a-zA-Z0-9_-]`).
pub fn catalog_id(server: &str, remote: &str) -> String {
    format!("mcp_{}_{}", sanitize(server), sanitize(remote))
}

/// Inverse of [`catalog_id`] for the remote tool name (server names are
/// unique per manager, so the suffix is unambiguous).
fn remote_name_of(catalog_id: &str, server: &str) -> String {
    catalog_id
        .strip_prefix(&format!("mcp_{}_", sanitize(server)))
        .unwrap_or(catalog_id)
        .to_owned()
}

fn sanitize(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}
