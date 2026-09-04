//! Vistalith-native tools (`agentic/TOOLS-PERMISSIONS.md`).
//!
//! Tools are Vistalith-owned: descriptors carry a consequence class, and the
//! registry resolves permissions (deny/allow) before execution — read-only
//! tools run, anything else is denied until scoped grants exist (later slice).
//! Every invocation is recorded as a durable `ToolInvoked` event by the
//! conversation engine, never flattened into prose (SPEC-007).

use serde_json::Value;
use thiserror::Error;
use vistalith_graph::SemanticWorldGraph;

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
    pub id: &'static str,
    pub description: &'static str,
    pub consequence: Consequence,
    /// JSON Schema of the tool arguments, sent to the model.
    pub parameters: Value,
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("unknown tool `{0}`")]
    UnknownTool(String),
    #[error("tool `{0}` requires a permission grant (consequence: write)")]
    PermissionDenied(String),
    #[error("invalid tool arguments for `{0}`: {1}")]
    InvalidArguments(String, String),
    #[error("tool `{0}` failed: {1}")]
    Failed(String, String),
}

/// A Vistalith-native tool. Implementations read the SWG they are given.
pub trait NativeTool: Send + Sync {
    fn descriptor(&self) -> &ToolDescriptor;
    fn execute(&self, graph: &SemanticWorldGraph, args: &Value) -> Result<Value, ToolError>;
}

/// Permission resolution for a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Allow,
    Deny,
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Box<dyn NativeTool>>,
}

impl ToolRegistry {
    /// The slice-4 registry: one native, read-only tool.
    pub fn graph_search() -> Self {
        ToolRegistry {
            tools: vec![Box::new(GraphSearchTool)],
        }
    }

    pub fn get(&self, id: &str) -> Option<&dyn NativeTool> {
        self.tools
            .iter()
            .find(|t| t.descriptor().id == id)
            .map(|t| t.as_ref())
    }

    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools.iter().map(|t| t.descriptor().clone()).collect()
    }

    /// Permission outcome for invoking `id`. Vistalith permissions restrict;
    /// they never weaken SDDK policy for SDDK-governed effects.
    pub fn permission(&self, id: &str) -> Result<Permission, ToolError> {
        let tool = self
            .get(id)
            .ok_or_else(|| ToolError::UnknownTool(id.to_owned()))?;
        Ok(match tool.descriptor().consequence {
            Consequence::ReadOnly => Permission::Allow,
            Consequence::Write | Consequence::Destructive => Permission::Deny,
        })
    }

    /// Resolves permission, then executes. The caller records the durable
    /// `ToolInvoked` event.
    pub fn invoke(
        &self,
        graph: &SemanticWorldGraph,
        id: &str,
        args: &Value,
    ) -> Result<Value, ToolError> {
        match self.permission(id)? {
            Permission::Deny => Err(ToolError::PermissionDenied(id.to_owned())),
            Permission::Allow => {
                let tool = self.get(id).expect("checked above");
                tool.execute(graph, args)
            }
        }
    }
}

/// `graph_search`: search SWG subjects by identity substring, optionally
/// filtered by namespace or kind. Read-only: it never mutates the graph.
pub struct GraphSearchTool;

impl NativeTool for GraphSearchTool {
    fn descriptor(&self) -> &ToolDescriptor {
        static DESCRIPTOR: std::sync::OnceLock<ToolDescriptor> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| ToolDescriptor {
            id: "graph_search",
            description: "Search the Semantic World Graph for subjects by an identity \
                          substring (matches namespace, kind or id), optionally filtered. \
                          Returns matching subjects with their authority class.",
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
        })
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
