//! MCP client integration (`agentic/MCP.md`, SPEC-009, ADR-009).
//!
//! Vistalith is an MCP *client*: it connects to external tool servers over
//! stdio (child process) or Streamable HTTP through the official Rust SDK
//! (`rmcp`, direct — no façade), discovers their tools, and projects them
//! into the unified tool catalog (`crate::tools`) where Vistalith
//! permissions apply to every call. Credentials never cross the client
//! boundary into any renderer (SPEC-008).
//!
//! Server model (`agentic/MCP.md`): health, reconnect, tools re-discovery
//! and enabled/disabled are first-class — a connection reports liveness,
//! re-spawns its child process (or re-opens the HTTP session) when a call
//! finds the transport closed, re-runs discovery on demand, and can be
//! disabled so its tools leave the unified catalog without being
//! unregistered. Remote servers authenticate with a bearer token or a
//! static header (`McpAuth`), resolved at connect/reconnect time; secrets
//! surface as a redacted *kind* in status/health and never in errors or
//! logs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use rmcp::model::{CallToolRequestParams, ClientInfo, ContentBlock, ToolAnnotations};
use rmcp::service::{RoleClient, RunningService, serve_client};
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
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

/// Static auth for a Streamable HTTP MCP server (SPK-007). The secret can
/// be inline (`token`/`value`) or environment-referenced
/// (`token_env`/`value_env`, resolved at connect/reconnect time). Status
/// and health expose only the redacted [`McpAuth::kind`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum McpAuth {
    /// `Authorization: Bearer <token>`.
    Bearer {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_env: Option<String>,
    },
    /// Arbitrary static header (e.g. `x-api-key`).
    Header {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_env: Option<String>,
    },
}

impl McpAuth {
    /// Resolves to the header (name, value). Error messages name the env
    /// variable, never the secret.
    pub fn resolve(&self) -> Result<(String, String), String> {
        match self {
            McpAuth::Bearer { token, token_env } => {
                let value = match (token, token_env) {
                    (Some(token), _) if !token.is_empty() => token.clone(),
                    (_, Some(env)) => std::env::var(env)
                        .map_err(|_| format!("auth env var `{env}` is not set"))?,
                    _ => return Err("bearer auth needs `token` or `token_env`".to_owned()),
                };
                Ok(("Authorization".to_owned(), format!("Bearer {value}")))
            }
            McpAuth::Header {
                name,
                value,
                value_env,
            } => {
                let resolved = match (value, value_env) {
                    (Some(value), _) if !value.is_empty() => value.clone(),
                    (_, Some(env)) => std::env::var(env)
                        .map_err(|_| format!("auth env var `{env}` is not set"))?,
                    _ => return Err("header auth needs `value` or `value_env`".to_owned()),
                };
                Ok((name.clone(), resolved))
            }
        }
    }

    /// Redacted description for status/health (never the secret).
    pub fn kind(&self) -> String {
        match self {
            McpAuth::Bearer { .. } => "bearer".to_owned(),
            McpAuth::Header { name, .. } => format!("header:{name}"),
        }
    }
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
    /// Static auth for the HTTP transport (SPK-007). Rejected on stdio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<McpAuth>,
}

impl McpServerConfig {
    pub fn validate(&self) -> Result<(), McpError> {
        if self.name.trim().is_empty() {
            return Err(McpError::InvalidConfig("missing `name`".to_owned()));
        }
        match (&self.command, &self.url) {
            (Some(_), None) => {
                if self.auth.is_some() {
                    return Err(McpError::InvalidConfig(format!(
                        "server `{}` sets `auth` on a stdio transport; auth applies to Streamable HTTP only",
                        self.name
                    )));
                }
                Ok(())
            }
            (None, Some(_)) => Ok(()),
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

/// Connection liveness (the health question of `agentic/MCP.md`).
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
    /// Disabled servers stay registered but their tools leave the catalog.
    pub disabled: bool,
    /// Redacted auth description (`bearer`, `header:x-api-key`) — the
    /// secret itself never leaves the process (SPEC-008).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
}

/// The live half of a connection: the negotiated session plus the tools it
/// advertised. Behind a mutex so reconnect/refresh can swap them in place.
struct ConnectionInner {
    service: RunningService<RoleClient, ClientInfo>,
    descriptors: Vec<ToolDescriptor>,
}

/// One MCP client connection. Shared through an `Arc` across requests;
/// interior mutability supports reconnect and re-discovery.
pub struct McpConnection {
    config: McpServerConfig,
    inner: Mutex<ConnectionInner>,
}

impl McpConnection {
    /// Connects, negotiates the session and discovers tools.
    pub async fn connect(config: McpServerConfig) -> Result<Self, McpError> {
        config.validate()?;
        let inner = Self::open(&config).await?;
        Ok(McpConnection {
            config,
            inner: Mutex::new(inner),
        })
    }

    async fn open(config: &McpServerConfig) -> Result<ConnectionInner, McpError> {
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
                // Auth (SPK-007): a preconfigured reqwest client carries the
                // credential as a default header, so EVERY request of the
                // transport — initialize, tool calls and the SSE stream —
                // authenticates. Reconnect rebuilds the client from the same
                // config (env-referenced secrets re-resolve).
                let transport = match &config.auth {
                    Some(auth) => {
                        let (name, value) = auth.resolve().map_err(|e| {
                            McpError::Connect(config.name.clone(), e)
                        })?;
                        let header_name =
                            reqwest::header::HeaderName::from_bytes(name.as_bytes())
                                .map_err(|e| {
                                    McpError::Connect(
                                        config.name.clone(),
                                        format!("invalid auth header name `{name}`: {e}"),
                                    )
                                })?;
                        let header_value = reqwest::header::HeaderValue::from_str(&value)
                            .map_err(|e| {
                                McpError::Connect(
                                    config.name.clone(),
                                    format!("invalid auth header value: {e}"),
                                )
                            })?;
                        let mut headers = reqwest::header::HeaderMap::new();
                        headers.insert(header_name, header_value);
                        let client = reqwest::Client::builder()
                            .default_headers(headers)
                            .build()
                            .map_err(|e| {
                                McpError::Connect(config.name.clone(), e.to_string())
                            })?;
                        StreamableHttpClientTransport::with_client(
                            client,
                            StreamableHttpClientTransportConfig::with_uri(url.as_str()),
                        )
                    }
                    None => StreamableHttpClientTransport::from_uri(url.as_str()),
                };
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
        Ok(ConnectionInner { service, descriptors })
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    pub fn config(&self) -> &McpServerConfig {
        &self.config
    }

    pub fn status(&self) -> McpServerStatus {
        let inner = self.inner.lock().expect("mcp connection lock");
        McpServerStatus {
            name: self.config.name.clone(),
            transport: if self.config.command.is_some() {
                "stdio".to_owned()
            } else {
                "http".to_owned()
            },
            status: if inner.service.is_transport_closed() {
                ConnectionStatus::Unhealthy
            } else {
                ConnectionStatus::Connected
            },
            tools: inner.descriptors.len(),
            disabled: false,
            auth: self.config.auth.as_ref().map(McpAuth::kind),
        }
    }

    /// Catalog entries for this connection (ids namespaced by server name).
    /// Takes `&Arc<Self>` so every entry can clone the shared handle the
    /// manager holds.
    pub fn entries(self: &Arc<Self>) -> Vec<ToolEntry> {
        let descriptors = {
            let inner = self.inner.lock().expect("mcp connection lock");
            inner.descriptors.clone()
        };
        descriptors
            .iter()
            .map(|descriptor| {
                let remote = remote_name_of(&descriptor.id, &self.config.name);
                ToolEntry::mcp(descriptor.clone(), remote, Arc::clone(self))
            })
            .collect()
    }

    /// Calls a remote tool by its *original* name. If the transport died
    /// (child process exited, HTTP session dropped), the connection
    /// reconnects once and retries — the reconnect question of
    /// `agentic/MCP.md` answered in place.
    pub async fn call(&self, tool: &str, args: Value) -> Result<Value, ToolError> {
        match self.call_inner(tool, args.clone()).await {
            Ok(result) => Ok(result),
            Err(_err) if self.transport_closed() => {
                self.reconnect()
                    .await
                    .map_err(|e| ToolError::Failed(tool.to_owned(), e.to_string()))?;
                self.call_inner(tool, args).await
            }
            Err(err) => Err(err),
        }
    }

    fn transport_closed(&self) -> bool {
        self.inner
            .lock()
            .expect("mcp connection lock")
            .service
            .is_transport_closed()
    }

    async fn call_inner(&self, tool: &str, args: Value) -> Result<Value, ToolError> {
        let params = {
            let mut params = CallToolRequestParams::default();
            params.name = tool.to_owned().into();
            params.arguments = args.as_object().cloned();
            params
        };
        // The peer handle multiplexes calls over the session; clone it out
        // so the lock is never held across an await.
        let peer = {
            let inner = self.inner.lock().expect("mcp connection lock");
            inner.service.peer().clone()
        };
        let result = peer
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

    /// Re-discovers the server's tools (the `tools/list_changed` question):
    /// replaces the descriptor set and returns the new count.
    pub async fn refresh(&self) -> Result<usize, McpError> {
        let peer = {
            let inner = self.inner.lock().expect("mcp connection lock");
            inner.service.peer().clone()
        };
        let listed = peer
            .list_tools(None)
            .await
            .map_err(|e| McpError::Discovery(self.config.name.clone(), e.to_string()))?;
        let descriptors: Vec<ToolDescriptor> = listed
            .tools
            .iter()
            .map(|tool| mcp_tool_descriptor(&self.config.name, tool))
            .collect();
        let count = descriptors.len();
        self.inner.lock().expect("mcp connection lock").descriptors = descriptors;
        Ok(count)
    }

    /// Shuts the current session down and opens a fresh one (child process
    /// re-spawned / HTTP session re-negotiated), re-discovering tools.
    pub async fn reconnect(&self) -> Result<(), McpError> {
        let fresh = Self::open(&self.config).await?;
        let old = {
            let mut inner = self.inner.lock().expect("mcp connection lock");
            std::mem::replace(&mut *inner, fresh)
        };
        let _ = old.service.cancel().await;
        Ok(())
    }
}

/// Manager for the live MCP connections (`agentic/MCP.md`'s server model:
/// enabled/disabled + health + reconnect).
#[derive(Default)]
pub struct McpManager {
    connections: Mutex<BTreeMap<String, Arc<McpConnection>>>,
    disabled: Mutex<BTreeSet<String>>,
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
        self.disabled
            .lock()
            .expect("mcp manager lock")
            .remove(name);
        self.connections
            .lock()
            .expect("mcp manager lock")
            .remove(name)
    }

    pub fn get(&self, name: &str) -> Option<Arc<McpConnection>> {
        self.connections
            .lock()
            .expect("mcp manager lock")
            .get(name)
            .cloned()
    }

    /// Enabled/disabled (`agentic/MCP.md`): a disabled server keeps its
    /// registration and connection but its tools leave the unified catalog.
    pub fn set_disabled(&self, name: &str, disabled: bool) -> bool {
        if self.get(name).is_none() {
            return false;
        }
        let mut set = self.disabled.lock().expect("mcp manager lock");
        if disabled {
            set.insert(name.to_owned());
        } else {
            set.remove(name);
        }
        true
    }

    pub fn is_disabled(&self, name: &str) -> bool {
        self.disabled
            .lock()
            .expect("mcp manager lock")
            .contains(name)
    }

    /// Snapshots the live, ENABLED connections (for building the unified
    /// catalog — disabled servers' tools stay out).
    pub fn connections(&self) -> Vec<Arc<McpConnection>> {
        let disabled = self.disabled.lock().expect("mcp manager lock");
        self.connections
            .lock()
            .expect("mcp manager lock")
            .values()
            .filter(|connection| !disabled.contains(connection.name()))
            .cloned()
            .collect()
    }

    pub fn status(&self) -> Vec<McpServerStatus> {
        let disabled = self.disabled.lock().expect("mcp manager lock");
        let mut statuses: Vec<McpServerStatus> = self
            .connections
            .lock()
            .expect("mcp manager lock")
            .values()
            .map(|c| {
                let mut status = c.status();
                status.disabled = disabled.contains(&status.name);
                status
            })
            .collect();
        statuses.sort_by(|a, b| a.name.cmp(&b.name));
        statuses
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
