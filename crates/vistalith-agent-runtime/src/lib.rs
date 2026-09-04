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

pub mod conversation;
pub mod provider;

pub use conversation::{ConversationEngine, ConversationError, ThreadReply};
pub use provider::{
    ChatMessage, FakeProvider, ModelError, ModelProvider, ModelRequest, ModelResponse, RigProvider,
    RuntimeError, RuntimeProvider,
};
