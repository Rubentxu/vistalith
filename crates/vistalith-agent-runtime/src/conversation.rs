//! The first conversation runtime (IMPLEMENT-NOW.md item 10).
//!
//! Every step is durable: the user message, the assistant reply and the turn
//! completion are `VEvent`s projected into the SWG (SPEC-007). The engine
//! holds no state of its own — the log is the truth.

use thiserror::Error;
use uuid::Uuid;
use vistalith_domain::{
    Actor, EventPayload, MessageAppended, MessageRole, ModelUsage, SubjectKind, SubjectRef,
    ThreadForked, ThreadStarted, ToolInvoked, TurnCompleted, VEvent,
};
use vistalith_graph::GraphStore;

use crate::provider::{ChatMessage, ModelProvider, ModelRequest};
use crate::tools::ToolRegistry;

/// Safety bound for the tool-call loop (one native tool round-trips once).
const MAX_TOOL_ROUNDS: usize = 2;

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

/// Result of forking a thread (SPEC-011): the new durable thread, the turn
/// boundary it was cut at, and how many events were carried over.
#[derive(Debug, Clone)]
pub struct ForkedThread {
    pub fork: SubjectRef,
    pub source: SubjectRef,
    pub up_to_turn: u64,
    pub copied_events: usize,
}

pub struct ConversationEngine<P: ModelProvider> {
    provider: P,
    system: Option<String>,
    actor: Actor,
    tools: Option<ToolRegistry>,
}

impl<P: ModelProvider> ConversationEngine<P> {
    pub fn new(provider: P) -> Self {
        ConversationEngine {
            provider,
            system: None,
            actor: Actor::new("system:vistalithd").expect("static actor"),
            tools: None,
        }
    }

    /// Offers the registry's native tools to the model.
    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = Some(tools);
        self
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

        // Turn loop: at most `MAX_TOOL_ROUNDS` tool-call rounds, then the
        // final text answer. Every executed call is a durable typed item.
        let mut rounds = 0usize;
        let mut total_usage = vistalith_domain::ModelUsage::default();
        loop {
            let request = self.build_request(store, thread);
            let response = self.provider.complete(request).await?;
            total_usage.input_tokens += response.usage.input_tokens;
            total_usage.output_tokens += response.usage.output_tokens;
            total_usage.total_tokens += response.usage.total_tokens;

            if response.is_tool_call() && rounds < MAX_TOOL_ROUNDS {
                rounds += 1;
                let tool_results =
                    self.run_tool_calls(store, thread, turn, &response.tool_calls)?;
                // The tool outputs join the history as typed tool items; the
                // model sees them on the next round.
                let _ = tool_results;
                continue;
            }

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
                    usage: total_usage,
                }),
                vec![thread.clone()],
            ))?;

            return Ok(ThreadReply {
                thread: thread.clone(),
                message: assistant_message,
                turn,
                content: response.content,
                usage: total_usage,
            });
        }
    }

    /// Forks a thread at a turn boundary (SPEC-011). The fork is a new
    /// durable thread whose items are copied up to `up_to_turn` (default:
    /// the source's latest turn). Every copied item carries a
    /// `forked_of` binding to its original — semantic subject bindings are
    /// preserved by construction — and the fork links back to its source
    /// with a `forked_from` relation. Forks are advisory exploration
    /// state: promotion into SDDK stays an explicit, governed act.
    pub fn fork_thread(
        &self,
        store: &mut GraphStore,
        source: &SubjectRef,
        up_to_turn: Option<u64>,
        note: Option<String>,
    ) -> Result<ForkedThread, ConversationError> {
        let source_node = store
            .graph()
            .subject(source)
            .ok_or_else(|| ConversationError::UnknownThread(source.to_string()))?;
        let max_turn = source_node
            .properties
            .get("turns")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let limit = up_to_turn.unwrap_or(max_turn).min(max_turn);

        let fork = SubjectRef::new(
            vistalith_domain::Namespace::Agentic,
            SubjectKind::Thread,
            Uuid::now_v7().to_string(),
        )
        .expect("generated fork id is valid");

        // The forked event creates the fork thread subject (title derives
        // from the source during projection) and the forked_from relation.
        store.append(self.event(
            EventPayload::ThreadForked(ThreadForked {
                fork: fork.clone(),
                source: source.clone(),
                up_to_turn: limit,
                note,
            }),
            vec![fork.clone(), source.clone()],
        ))?;

        // Copy the durable items: messages, typed tool calls and turn
        // completions in log order, up to the requested turn. Tool items
        // inherit the turn of the message they followed (tool calls happen
        // mid-turn in this engine). The source events are snapshotted first
        // (the log is immutable history, so the copy is exact), then
        // re-appended adapted to the fork.
        let mut plan: Vec<CopyItem> = Vec::new();
        let mut current_turn = 0u64;
        for stored in store.log() {
            match &stored.event.payload {
                EventPayload::MessageAppended(appended)
                    if &appended.thread == source && appended.turn <= limit =>
                {
                    current_turn = appended.turn;
                    plan.push(CopyItem::Message {
                        original: appended.message.clone(),
                        role: appended.role,
                        content: appended.content.clone(),
                        turn: appended.turn,
                    });
                }
                EventPayload::ToolInvoked(invoked)
                    if &invoked.thread == source && current_turn <= limit =>
                {
                    plan.push(CopyItem::Tool {
                        original: invoked.tool_call.clone(),
                        tool: invoked.tool.clone(),
                        args: invoked.args.clone(),
                        output: invoked.output.clone(),
                    });
                }
                EventPayload::TurnCompleted(turn)
                    if &turn.thread == source && turn.turn <= limit =>
                {
                    plan.push(CopyItem::Turn {
                        turn: turn.turn,
                        model: turn.model.clone(),
                        usage: turn.usage,
                    });
                }
                _ => {}
            }
        }

        let copied = plan.len();
        for item in plan {
            match item {
                CopyItem::Message {
                    original,
                    role,
                    content,
                    turn,
                } => {
                    let message = SubjectRef::new(
                        vistalith_domain::Namespace::Agentic,
                        SubjectKind::Message,
                        Uuid::now_v7().to_string(),
                    )
                    .expect("generated forked message id is valid");
                    store.append(self.event(
                        EventPayload::MessageAppended(MessageAppended {
                            thread: fork.clone(),
                            message,
                            role,
                            content,
                            turn,
                            forked_of: Some(original.clone()),
                        }),
                        vec![fork.clone(), original],
                    ))?;
                }
                CopyItem::Tool {
                    original,
                    tool,
                    args,
                    output,
                } => {
                    let tool_call = SubjectRef::new(
                        vistalith_domain::Namespace::Agentic,
                        SubjectKind::ToolCall,
                        Uuid::now_v7().to_string(),
                    )
                    .expect("generated forked tool-call id is valid");
                    store.append(self.event(
                        EventPayload::ToolInvoked(ToolInvoked {
                            thread: fork.clone(),
                            tool_call,
                            tool,
                            args,
                            output,
                            forked_of: Some(original),
                        }),
                        vec![fork.clone()],
                    ))?;
                }
                CopyItem::Turn { turn, model, usage } => {
                    store.append(self.event(
                        EventPayload::TurnCompleted(TurnCompleted {
                            thread: fork.clone(),
                            turn,
                            model,
                            usage,
                        }),
                        vec![fork.clone()],
                    ))?;
                }
            }
        }

        Ok(ForkedThread {
            fork,
            source: source.clone(),
            up_to_turn: limit,
            copied_events: copied,
        })
    }

    fn build_request(&self, store: &GraphStore, thread: &SubjectRef) -> ModelRequest {
        ModelRequest {
            model: self.provider.descriptor().clone(),
            system: self.system.clone(),
            messages: thread_history(store, thread),
            max_tokens: Some(1024),
            tools: self.tool_contracts(),
        }
    }

    fn tool_contracts(&self) -> Vec<crate::provider::ToolContract> {
        self.tools
            .as_ref()
            .map(|registry| {
                registry
                    .descriptors()
                    .into_iter()
                    .map(|d| crate::provider::ToolContract {
                        name: d.id.to_owned(),
                        description: d.description.to_owned(),
                        parameters: d.parameters,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Executes tool calls requested by the model: permission check via the
    /// registry, run, and record one durable `ToolInvoked` item per call.
    fn run_tool_calls(
        &self,
        store: &mut GraphStore,
        thread: &SubjectRef,
        turn: u64,
        calls: &[crate::provider::ToolCallRequest],
    ) -> Result<Vec<serde_json::Value>, ConversationError> {
        let mut outputs = Vec::with_capacity(calls.len());
        for call in calls {
            let tool_call = SubjectRef::new(
                vistalith_domain::Namespace::Agentic,
                SubjectKind::ToolCall,
                Uuid::now_v7().to_string(),
            )
            .expect("generated tool call id is valid");
            let output = match self.tools.as_ref() {
                Some(registry) => {
                    let result = registry.invoke(store.graph(), &call.name, &call.arguments);
                    result.unwrap_or_else(|err| serde_json::json!({ "error": err.to_string() }))
                }
                None => serde_json::json!({ "error": "no tools registered" }),
            };
            store.append(self.event(
                EventPayload::ToolInvoked(ToolInvoked {
                    thread: thread.clone(),
                    tool_call: tool_call.clone(),
                    tool: call.name.clone(),
                    args: call.arguments.clone(),
                    output: output.clone(),
                    forked_of: None,
                }),
                vec![thread.clone(), tool_call],
            ))?;
            // The model sees the tool output as the next history item.
            self.append_message(
                store,
                thread,
                MessageRole::Tool,
                &serde_json::to_string(&output).unwrap_or_default(),
                turn,
            )?;
            outputs.push(output);
        }
        Ok(outputs)
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
                forked_of: None,
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

/// A source event selected for copying into a fork (snapshot of the
/// immutable log, adapted at re-append time).
enum CopyItem {
    Message {
        original: SubjectRef,
        role: MessageRole,
        content: String,
        turn: u64,
    },
    Tool {
        original: SubjectRef,
        tool: String,
        args: serde_json::Value,
        output: serde_json::Value,
    },
    Turn {
        turn: u64,
        model: vistalith_domain::ModelDescriptor,
        usage: ModelUsage,
    },
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
