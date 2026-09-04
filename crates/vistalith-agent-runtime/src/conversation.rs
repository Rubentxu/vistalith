//! The first conversation runtime (IMPLEMENT-NOW.md item 10).
//!
//! Every step is durable: the user message, the assistant reply and the turn
//! completion are `VEvent`s projected into the SWG (SPEC-007). The engine
//! holds no state of its own — the log is the truth.

use thiserror::Error;
use uuid::Uuid;
use vistalith_domain::{
    Actor, EventPayload, MessageAppended, MessageRole, ModelUsage, SubjectKind,
    SubjectRef, ThreadStarted, TurnCompleted, VEvent,
};
use vistalith_graph::GraphStore;

use crate::provider::{ChatMessage, ModelProvider, ModelRequest};

#[derive(Debug, Error)]
pub enum ConversationError {
    #[error("thread `{0}` does not exist")]
    UnknownThread(String),
    #[error(transparent)]
    Store(#[from] vistalith_graph::StoreError),
    #[error(transparent)]
    Model(#[from] crate::provider::ModelError),
    #[error("empty user message")]
    EmptyMessage,
}

/// A completed turn: the assistant reply plus its durable coordinates.
#[derive(Debug, Clone)]
pub struct ThreadReply {
    pub thread: SubjectRef,
    pub message: SubjectRef,
    pub turn: u64,
    pub content: String,
    pub usage: ModelUsage,
}

pub struct ConversationEngine<P: ModelProvider> {
    provider: P,
    system: Option<String>,
    actor: Actor,
}

impl<P: ModelProvider> ConversationEngine<P> {
    pub fn new(provider: P) -> Self {
        ConversationEngine {
            provider,
            system: None,
            actor: Actor::new("system:vistalithd").expect("static actor"),
        }
    }

    pub fn with_system_prompt(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    pub fn with_actor(mut self, actor: Actor) -> Self {
        self.actor = actor;
        self
    }

    /// The configured provider (tests use this to inspect recorded requests).
    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Starts a thread: durable `thread-started` event, `agentic:thread:<uuid>`
    /// subject in the SWG.
    pub fn start_thread(
        &self,
        store: &mut GraphStore,
        title: impl Into<String>,
    ) -> Result<SubjectRef, ConversationError> {
        let title = title.into();
        let thread = SubjectRef::new(
            vistalith_domain::Namespace::Agentic,
            SubjectKind::Thread,
            Uuid::now_v7().to_string(),
        )
        .expect("generated thread id is valid");
        store.append(self.event(
            EventPayload::ThreadStarted(ThreadStarted {
                thread: thread.clone(),
                title,
            }),
            vec![thread.clone()],
        ))?;
        Ok(thread)
    }

    /// Appends a user message and runs one turn: provider completion, durable
    /// assistant message, durable `turn-completed` with usage.
    pub async fn send_user_message(
        &self,
        store: &mut GraphStore,
        thread: &SubjectRef,
        content: impl Into<String>,
    ) -> Result<ThreadReply, ConversationError> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(ConversationError::EmptyMessage);
        }
        if store.graph().subject(thread).is_none() {
            return Err(ConversationError::UnknownThread(thread.to_string()));
        }

        let turn = next_turn(store, thread);
        self.append_message(store, thread, MessageRole::User, &content, turn)?;

        let history = thread_history(store, thread);
        let request = ModelRequest {
            model: self.provider.descriptor().clone(),
            system: self.system.clone(),
            messages: history,
            max_tokens: Some(1024),
        };
        let response = self.provider.complete(request).await?;

        let assistant_message = self.append_message(
            store,
            thread,
            MessageRole::Assistant,
            &response.content,
            turn,
        )?;

        store.append(self.event(
            EventPayload::TurnCompleted(TurnCompleted {
                thread: thread.clone(),
                turn,
                model: response.model,
                usage: response.usage,
            }),
            vec![thread.clone()],
        ))?;

        Ok(ThreadReply {
            thread: thread.clone(),
            message: assistant_message,
            turn,
            content: response.content,
            usage: response.usage,
        })
    }

    fn append_message(
        &self,
        store: &mut GraphStore,
        thread: &SubjectRef,
        role: MessageRole,
        content: &str,
        turn: u64,
    ) -> Result<SubjectRef, ConversationError> {
        let message = SubjectRef::new(
            vistalith_domain::Namespace::Agentic,
            SubjectKind::Message,
            Uuid::now_v7().to_string(),
        )
        .expect("generated message id is valid");
        store.append(self.event(
            EventPayload::MessageAppended(MessageAppended {
                thread: thread.clone(),
                message: message.clone(),
                role,
                content: content.to_owned(),
                turn,
            }),
            vec![thread.clone(), message.clone()],
        ))?;
        Ok(message)
    }

    fn event(&self, payload: EventPayload, subjects: Vec<SubjectRef>) -> VEvent {
        VEvent {
            event_id: Uuid::now_v7(),
            actor: self.actor.clone(),
            timestamp: time::OffsetDateTime::now_utc(),
            subjects,
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            trace_id: None,
            payload,
        }
    }
}

fn next_turn(store: &GraphStore, thread: &SubjectRef) -> u64 {
    store
        .graph()
        .subject(thread)
        .and_then(|node| node.properties.get("turns"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        + 1
}

/// Reconstructs the conversation history from the SWG projection, in turn
/// order. Includes the just-appended user prompt as the final message
/// (SPEC-007: reconstruction comes from durable state alone).
fn thread_history(store: &GraphStore, thread: &SubjectRef) -> Vec<ChatMessage> {
    let mut messages: Vec<(u64, u64, ChatMessage)> = store
        .graph()
        .children(thread)
        .into_iter()
        .filter_map(|node| {
            let role = node.properties.get("role")?;
            let content = node.properties.get("content")?.as_str()?;
            let turn_no = node
                .properties
                .get("turn")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let role = serde_json::from_value::<MessageRole>(role.clone()).ok()?;
            Some((
                turn_no,
                node.last_event_sequence,
                ChatMessage {
                    role,
                    content: content.to_owned(),
                },
            ))
        })
        .collect();
    messages.sort_by_key(|(turn_no, sequence, _)| (*turn_no, *sequence));
    messages
        .into_iter()
        .map(|(_, _, message)| message)
        .collect()
}
