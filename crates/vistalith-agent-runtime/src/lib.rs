//! Vistalith agent runtime (slice 3): the first conversation thread and one
//! LLM provider behind Vistalith-owned contracts.
//!
//! Rules (`agentic/RIG-STRATEGY.md`, SPEC-008, ADR-008):
//! - Vistalith owns `ModelRequest`/`ModelResponse`/`ModelDescriptor`/`ModelUsage`
//!   and the [`provider::ModelProvider`] contract; Rig is an adapter underneath
//!   and its types never cross this crate's public surface.
//! - Conversations are durable Vistalith state (SPEC-007): every user message,
//!   assistant reply and turn completion is a `VEvent` projected into the SWG.
//! - Model calls are live non-deterministic behavior; the [`FakeProvider`]
//!   gives recorded-external determinism for tests and offline demos.
//! - Tools (native + MCP, SPEC-009 / `agentic/TOOLS-PERMISSIONS.md`) project
//!   into one catalog behind a deny/allow/ask permission gate with scoped
//!   temporary grants; rmcp is consumed directly (ADR-009).

pub mod conversation;
pub mod frames;
pub mod intents;
pub mod mcp;
pub mod provider;
pub mod tools;

pub use conversation::{ConversationEngine, ConversationError, ForkedThread, ThreadReply};
pub use frames::{
    FrameError, FrameSpec, FrameTurnReport, close_frame, define_agent, finish_agent_run,
    frame_system_prompt, frame_thread, run_frame_turn, start_agent_frame, start_frame,
};
pub use vistalith_domain::FrameOutcome;
pub use intents::{
    IntentError, Promotion, discard_intent, draft_intent, promote_intent,
    promote_intent_with_bridge,
};
pub use mcp::{
    ConnectionStatus, McpAuth, McpConnection, McpError, McpManager, McpServerConfig,
    McpServerStatus, catalog_id, consequence_from_annotations,
};
pub use provider::{
    ChatMessage, FakeProvider, FakeStep, ModelError, ModelEvent, ModelEventRx, ModelProvider,
    ModelRequest, ModelResponse, RigProvider, RuntimeError, RuntimeProvider, ToolCallRequest,
    ToolContract,
};
pub use tools::{
    Consequence, Grant, GrantStore, GraphSearchTool, NativeTool, PermissionDecision, ToolDescriptor,
    ToolEntry, ToolError, ToolRegistry, ToolSource,
};
