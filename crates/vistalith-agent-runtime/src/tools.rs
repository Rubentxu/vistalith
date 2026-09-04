//! Vistalith-native tools (`agentic/TOOLS-PERMISSIONS.md`).
//!
//! One unified catalog: native tools and MCP tools project into the same
//! registry (`agentic/MCP.md`), every entry carrying a consequence class and
//! its source. Permission outcomes are deny / allow / ask, with **scoped
//! temporary grants**: a write/destructive tool runs only while a grant with
//! remaining calls exists, and each authorized call consumes one. Vistalith
//! permissions restrict; they never weaken SDDK policy for SDDK-governed
//! effects. Every invocation is recorded as a durable `ToolInvoked` event by
//! the conversation engine, never flattened into prose (SPEC-007).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use vistalith_graph::SemanticWorldGraph;

use crate::mcp::McpConnection;

/// Where a tool comes from (`agentic/TOOLS-PERMISSIONS.md`: descriptor
/// `source`). The wire label is durable in `ToolInvoked::source`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ToolSource {
    Native,
    Mcp { server: String },
}

impl ToolSource {
    pub fn label(&self) -> String {
        match self {
            ToolSource::Native => "native".to_owned(),
            ToolSource::Mcp { server } => format!("mcp:{server}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Consequence {
    ReadOnly,
    Write,
    Destructive,
}

/// Vistalith-owned tool descriptor (`agentic/TOOLS-PERMISSIONS.md`).
#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub id: String,
    pub description: String,
    pub consequence: Consequence,
    /// JSON Schema of the tool arguments, sent to the model.
    pub parameters: Value,
    pub source: ToolSource,
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("unknown tool `{0}`")]
    UnknownTool(String),
    #[error("tool `{0}` is denied by policy")]
    PermissionDenied(String),
    #[error("tool `{0}` requires a scoped permission grant (consequence: write)")]
    ApprovalRequired(String),
    #[error("invalid tool arguments for `{0}`: {1}")]
    InvalidArguments(String, String),
    #[error("tool `{0}` failed: {1}")]
    Failed(String, String),
}

/// A Vistalith-native tool. Implementations read the SWG they are given.
pub trait NativeTool: Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;
    fn execute(&self, graph: &SemanticWorldGraph, args: &Value) -> Result<Value, ToolError>;
}

/// Permission outcomes (`agentic/TOOLS-PERMISSIONS.md`): deny / allow / ask.
/// `Ask` inside an automated turn means the call does not run until a scoped
/// grant exists; the denial is recorded and surfaced over the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionDecision {
    Allow,
    Ask,
    Deny,
}

/// One catalog entry: the descriptor plus how to reach the tool.
#[derive(Clone)]
pub struct ToolEntry {
    pub descriptor: ToolDescriptor,
    /// MCP only: the tool name on the remote server (ids in the catalog are
    /// namespaced, remote names are not).
    pub remote_name: Option<String>,
    pub(crate) backend: ToolBackend,
}

impl ToolEntry {
    /// Catalog entry for one discovered MCP tool.
    pub fn mcp(
        descriptor: ToolDescriptor,
        remote_name: String,
        connection: Arc<McpConnection>,
    ) -> Self {
        ToolEntry {
            descriptor,
            remote_name: Some(remote_name),
            backend: ToolBackend::Mcp(connection),
        }
    }
}

#[derive(Clone)]
pub enum ToolBackend {
    Native(Arc<dyn NativeTool>),
    Mcp(Arc<McpConnection>),
}

/// Scoped temporary grants (`agentic/TOOLS-PERMISSIONS.md`): per-tool
/// remaining-call counters, shared across requests so a grant made over the
/// API governs later turns. Explicit denies always win.
#[derive(Default)]
pub struct GrantStore {
    grants: Mutex<HashMap<String, u32>>,
    denied: Mutex<std::collections::HashSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Grant {
    pub tool: String,
    pub remaining: u32,
}

impl GrantStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Grants `calls` more authorized invocations (replacing any previous
    /// grant for the tool).
    pub fn grant(&self, tool: &str, calls: u32) -> Grant {
        let mut grants = self.grants.lock().expect("grant lock");
        let remaining = grants.entry(tool.to_owned()).or_insert(0);
        *remaining = calls;
        Grant {
            tool: tool.to_owned(),
            remaining: calls,
        }
    }

    pub fn revoke(&self, tool: &str) -> bool {
        self.grants.lock().expect("grant lock").remove(tool).is_some()
    }

    pub fn set_denied(&self, tool: &str, denied: bool) {
        let mut set = self.denied.lock().expect("deny lock");
        if denied {
            set.insert(tool.to_owned());
        } else {
            set.remove(tool);
        }
    }

    pub fn is_denied(&self, tool: &str) -> bool {
        self.denied.lock().expect("deny lock").contains(tool)
    }

    /// Consumes one authorized call. Authorization happens before execution:
    /// a failed execution still consumed the grant, which bounds retries.
    fn consume(&self, tool: &str) -> bool {
        let mut grants = self.grants.lock().expect("grant lock");
        match grants.get_mut(tool) {
            Some(remaining) if *remaining > 0 => {
                *remaining -= 1;
                true
            }
            _ => false,
        }
    }

    pub fn remaining(&self, tool: &str) -> u32 {
        self.grants
            .lock()
            .expect("grant lock")
            .get(tool)
            .copied()
            .unwrap_or(0)
    }

    pub fn all(&self) -> Vec<Grant> {
        let grants = self.grants.lock().expect("grant lock");
        let mut out: Vec<Grant> = grants
            .iter()
            .map(|(tool, remaining)| Grant {
                tool: tool.clone(),
                remaining: *remaining,
            })
            .collect();
        out.sort_by(|a, b| a.tool.cmp(&b.tool));
        out
    }
}

/// The unified tool catalog (`agentic/MCP.md`): native + MCP tools behind
/// one permission gate.
pub struct ToolRegistry {
    tools: Vec<ToolEntry>,
    grants: Arc<GrantStore>,
}

impl ToolRegistry {
    /// Registry with only the Vistalith-native tools.
    pub fn native(grants: Arc<GrantStore>) -> Self {
        let mut registry = ToolRegistry {
            tools: Vec::new(),
            grants,
        };
        registry.add_native(Box::new(GraphSearchTool));
        registry
    }

    pub fn add_native(&mut self, tool: Box<dyn NativeTool>) {
        self.tools.push(ToolEntry {
            descriptor: tool.descriptor(),
            remote_name: None,
            backend: ToolBackend::Native(Arc::from(tool)),
        });
    }

    /// Projects one MCP connection's discovered tools into the catalog.
    pub fn add_mcp(&mut self, connection: Arc<McpConnection>) {
        for entry in connection.entries() {
            self.tools.push(entry);
        }
    }

    pub fn get(&self, id: &str) -> Option<&ToolEntry> {
        self.tools.iter().find(|t| t.descriptor.id == id)
    }

    /// A registry whose catalog is the intersection with `allowed` (frame
    /// `permitted_tools`). Grants are shared: a grant made outside a frame
    /// still governs inside it — frames bound tools, they never weaken the
    /// permission gate.
    pub fn restricted_to(&self, allowed: &[String]) -> ToolRegistry {
        let tools: Vec<ToolEntry> = self
            .tools
            .iter()
            .filter(|entry| allowed.contains(&entry.descriptor.id))
            .cloned()
            .collect();
        ToolRegistry {
            tools,
            grants: Arc::clone(&self.grants),
        }
    }

    /// The unified catalog (descriptors only, ordered by id).
    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        let mut out: Vec<ToolDescriptor> =
            self.tools.iter().map(|t| t.descriptor.clone()).collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    pub fn grants(&self) -> Arc<GrantStore> {
        Arc::clone(&self.grants)
    }

    /// Permission outcome for invoking `id`, resolved in order: explicit
    /// deny > consequence class > scoped grant. Ask is the resting state of
    /// every write-class tool without a live grant.
    pub fn permission(&self, id: &str) -> Result<PermissionDecision, ToolError> {
        let entry = self
            .get(id)
            .ok_or_else(|| ToolError::UnknownTool(id.to_owned()))?;
        if self.grants.is_denied(id) {
            return Ok(PermissionDecision::Deny);
        }
        Ok(match entry.descriptor.consequence {
            Consequence::ReadOnly => PermissionDecision::Allow,
            Consequence::Write | Consequence::Destructive => {
                if self.grants.remaining(id) > 0 {
                    PermissionDecision::Allow
                } else {
                    PermissionDecision::Ask
                }
            }
        })
    }

    /// Resolves permission, then executes. The caller records the durable
    /// `ToolInvoked` event either way.
    pub async fn invoke(
        &self,
        graph: &SemanticWorldGraph,
        id: &str,
        args: &Value,
    ) -> Result<Value, ToolError> {
        let entry = self
            .get(id)
            .ok_or_else(|| ToolError::UnknownTool(id.to_owned()))?;
        if self.grants.is_denied(id) {
            return Err(ToolError::PermissionDenied(id.to_owned()));
        }
        match entry.descriptor.consequence {
            Consequence::ReadOnly => {}
            Consequence::Write | Consequence::Destructive => {
                if !self.grants.consume(id) {
                    return Err(ToolError::ApprovalRequired(id.to_owned()));
                }
            }
        }
        match &entry.backend {
            ToolBackend::Native(tool) => tool.execute(graph, args),
            ToolBackend::Mcp(connection) => {
                let remote = entry
                    .remote_name
                    .clone()
                    .unwrap_or_else(|| entry.descriptor.id.clone());
                connection.call(&remote, args.clone()).await
            }
        }
    }
}

/// `graph_search`: search SWG subjects by identity substring, optionally
/// filtered by namespace or kind. Read-only: it never mutates the graph.
pub struct GraphSearchTool;

impl NativeTool for GraphSearchTool {
    fn descriptor(&self) -> ToolDescriptor {
        static DESCRIPTOR: std::sync::OnceLock<ToolDescriptor> = std::sync::OnceLock::new();
        DESCRIPTOR
            .get_or_init(|| ToolDescriptor {
                id: "graph_search".to_owned(),
                description: "Search the Semantic World Graph for subjects by an identity \
                              substring (matches namespace, kind or id), optionally filtered. \
                              Returns matching subjects with their authority class."
                    .to_owned(),
                consequence: Consequence::ReadOnly,
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "substring to search for" },
                        "namespace": { "type": "string", "description": "optional namespace filter" },
                        "kind": { "type": "string", "description": "optional kind filter" },
                        "limit": { "type": "integer", "description": "max results (default 8)" }
                    },
                    "required": ["query"]
                }),
                source: ToolSource::Native,
            })
            .clone()
    }

    fn execute(&self, graph: &SemanticWorldGraph, args: &Value) -> Result<Value, ToolError> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidArguments("graph_search".into(), "missing `query` string".into())
            })?
            .to_lowercase();
        let namespace = args.get("namespace").and_then(Value::as_str);
        let kind = args.get("kind").and_then(Value::as_str);
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(8)
            .min(50) as usize;

        let matches: Vec<Value> = graph
            .subjects()
            .filter(|node| {
                let identity = node.subject.to_string().to_lowercase();
                if !identity.contains(&query) {
                    return false;
                }
                namespace.is_none_or(|ns| node.subject.namespace().as_str() == ns)
                    && kind.is_none_or(|k| node.subject.kind().as_str() == k)
            })
            .take(limit)
            .map(|node| {
                serde_json::json!({
                    "subject": node.subject.to_string(),
                    "authority": node.authority,
                    "name": node.properties.get("name").cloned().unwrap_or(Value::Null),
                    "deprecated": node.deprecated,
                })
            })
            .collect();

        Ok(serde_json::json!({ "matches": matches, "count": matches.len() }))
    }
}
