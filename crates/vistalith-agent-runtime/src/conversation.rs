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

#[derive(Debug, Clone, Error)]
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
    pub model: vistalith_domain::ModelDescriptor,
    pub usage: ModelUsage,
    /// Semantic subjects the USER message mentioned via `@ns:kind:id`
    /// (VIS-CHAT-004) — resolved identities only; unknown refs are
    /// reported in `unresolved_mentions` and never bound.
    pub mentions: Vec<SubjectRef>,
    pub unresolved_mentions: Vec<String>,
}

/// Per-turn overrides (slice 23, V5): a chat message may pick a model
/// (`provider/model`, must share the server's provider) and/or ride on an
/// agent definition (its instructions become the system prompt, its model
/// the target). `None` fields keep the engine defaults.
#[derive(Debug, Clone, Default)]
pub struct TurnOverrides {
    pub model: Option<vistalith_domain::ModelDescriptor>,
    pub system: Option<String>,
}

/// Parses `@namespace:kind:id` mention references out of message content
/// (VIS-CHAT-004). Segments accept the SubjectRef identifier charset
/// (`[a-zA-Z0-9_-]`); duplicates are removed preserving first occurrence.
/// Parsing is purely lexical — existence is the caller's concern.
pub fn parse_mention_refs(content: &str) -> Vec<SubjectRef> {
    const IDENT: fn(char) -> bool =
        |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';

    fn read_segment(token: &str) -> Option<(&str, &str)> {
        let end = token
            .char_indices()
            .find(|(_, c)| !IDENT(*c))
            .map(|(index, _)| index)
            .unwrap_or(token.len());
        let (segment, rest) = token.split_at(end);
        if segment.is_empty() {
            None
        } else {
            Some((segment, rest))
        }
    }

    let mut refs = Vec::new();
    for (at_index, _) in content.match_indices('@') {
        let token = &content[at_index + 1..];
        let Some((namespace, rest)) = read_segment(token) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        let Some((kind, rest)) = read_segment(rest) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        let Some((id, _)) = read_segment(rest) else {
            continue;
        };
        if let Ok(reference) = SubjectRef::parse(&format!("{namespace}:{kind}:{id}"))
            && !refs.contains(&reference)
        {
            refs.push(reference);
        }
    }
    refs
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

pub struct ConversationEngine<P: ModelProvider + Sync> {
    provider: P,
    system: Option<String>,
    actor: Actor,
    tools: Option<ToolRegistry>,
}

impl<P: ModelProvider + Sync> ConversationEngine<P> {
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
        self.send_user_message_opts(
            store,
            thread,
            content,
            TurnOverrides::default(),
        )
        .await
    }

    /// [`Self::send_user_message`] with per-turn overrides (model / system
    /// prompt) and `@ns:kind:id` mention resolution on the user message.
    pub async fn send_user_message_opts(
        &self,
        store: &mut GraphStore,
        thread: &SubjectRef,
        content: impl Into<String>,
        overrides: TurnOverrides,
    ) -> Result<ThreadReply, ConversationError> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(ConversationError::EmptyMessage);
        }
        if store.graph().subject(thread).is_none() {
            return Err(ConversationError::UnknownThread(thread.to_string()));
        }

        let turn = next_turn(store, thread);
        let (mentions, unresolved_mentions) = {
            let (resolved, unresolved) =
                self.resolve_mentions(store, parse_mention_refs(&content));
            (resolved, unresolved)
        };
        self.append_message(
            store,
            thread,
            MessageRole::User,
            &content,
            turn,
            mentions.clone(),
        )?;

        // Turn loop: at most `MAX_TOOL_ROUNDS` tool-call rounds, then the
        // final text answer. Every executed call is a durable typed item.
        let mut rounds = 0usize;
        let mut total_usage = vistalith_domain::ModelUsage::default();
        loop {
            let request = self.build_request(store, thread, &overrides);
            let response = self.provider.complete(request).await?;
            total_usage.input_tokens += response.usage.input_tokens;
            total_usage.output_tokens += response.usage.output_tokens;
            total_usage.total_tokens += response.usage.total_tokens;

            if response.is_tool_call() && rounds < MAX_TOOL_ROUNDS {
                rounds += 1;
                let tool_results =
                    self.run_tool_calls(store, thread, turn, &response.tool_calls).await?;
                // The tool outputs join the history as typed tool items; the
                // model sees them on the next round.
                let _ = tool_results;
                continue;
            }

            let model = response.model.clone();
            let assistant_message = self.append_message(
                store,
                thread,
                MessageRole::Assistant,
                &response.content,
                turn,
                Vec::new(),
            )?;

            store.append(self.event(
                EventPayload::TurnCompleted(TurnCompleted {
                    thread: thread.clone(),
                    turn,
                    model: model.clone(),
                    usage: total_usage,
                }),
                vec![thread.clone()],
            ))?;

            return Ok(ThreadReply {
                thread: thread.clone(),
                message: assistant_message,
                turn,
                content: response.content,
                model,
                usage: total_usage,
                mentions,
                unresolved_mentions,
            });
        }
    }

    /// Streams a turn: identical durability to [`Self::send_user_message`]
    /// (the same durable events, appended at the same points), but the final
    /// model completion is streamed and text deltas are forwarded through
    /// `deltas` as they arrive. Tool rounds still run non-streamed first.
    pub async fn send_user_message_streaming(
        &self,
        store: &mut GraphStore,
        thread: &SubjectRef,
        content: impl Into<String>,
        deltas: tokio::sync::mpsc::Sender<String>,
    ) -> Result<ThreadReply, ConversationError> {
        self.send_user_message_streaming_opts(
            store,
            thread,
            content,
            deltas,
            TurnOverrides::default(),
        )
        .await
    }

    /// [`Self::send_user_message_streaming`] with per-turn overrides and
    /// `@ns:kind:id` mention resolution — identical durability.
    pub async fn send_user_message_streaming_opts(
        &self,
        store: &mut GraphStore,
        thread: &SubjectRef,
        content: impl Into<String>,
        deltas: tokio::sync::mpsc::Sender<String>,
        overrides: TurnOverrides,
    ) -> Result<ThreadReply, ConversationError> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(ConversationError::EmptyMessage);
        }
        if store.graph().subject(thread).is_none() {
            return Err(ConversationError::UnknownThread(thread.to_string()));
        }

        let turn = next_turn(store, thread);
        let (mentions, unresolved_mentions) =
            self.resolve_mentions(store, parse_mention_refs(&content));
        self.append_message(
            store,
            thread,
            MessageRole::User,
            &content,
            turn,
            mentions.clone(),
        )?;

        let mut rounds = 0usize;
        let mut total_usage = vistalith_domain::ModelUsage::default();
        loop {
            // Every round streams: deltas forward live; the terminal event
            // decides between a final answer and tool calls (which run and
            // loop, exactly like the non-streamed path).
            let request = self.build_request(store, thread, &overrides);
            let mut events = self.provider.stream_complete(request).await?;
            let mut finished: Option<(
                String,
                vistalith_domain::ModelDescriptor,
                vistalith_domain::ModelUsage,
                Vec<crate::provider::ToolCallRequest>,
            )> = None;
            let mut tool_round_ran = false;
            while let Some(event) = events.recv().await {
                match event? {
                    crate::provider::ModelEvent::Delta { text } => {
                        let _ = deltas.send(text).await;
                    }
                    crate::provider::ModelEvent::Finished {
                        content,
                        model,
                        usage,
                        tool_calls,
                    } => {
                        total_usage.input_tokens += usage.input_tokens;
                        total_usage.output_tokens += usage.output_tokens;
                        total_usage.total_tokens += usage.total_tokens;
                        if !tool_calls.is_empty() && rounds < MAX_TOOL_ROUNDS {
                            rounds += 1;
                            tool_round_ran = true;
                            self.run_tool_calls(store, thread, turn, &tool_calls).await?;
                        } else {
                            finished = Some((content, model, usage, tool_calls));
                        }
                    }
                }
                if finished.is_some() {
                    break;
                }
            }
            if tool_round_ran {
                // Tool outputs went back to the model: stream the next round.
                continue;
            }
            // Usage was already accumulated from the terminal event above.
            let Some((content, model, _, _)) = finished else {
                return Err(ConversationError::Model(
                    crate::provider::ModelError::EmptyResponse,
                ));
            };

            let assistant_message =
                self.append_message(
                    store,
                    thread,
                    MessageRole::Assistant,
                    &content,
                    turn,
                    Vec::new(),
                )?;
            store.append(self.event(
                EventPayload::TurnCompleted(TurnCompleted {
                    thread: thread.clone(),
                    turn,
                    model: model.clone(),
                    usage: total_usage,
                }),
                vec![thread.clone()],
            ))?;

            return Ok(ThreadReply {
                thread: thread.clone(),
                message: assistant_message,
                turn,
                content,
                model,
                usage: total_usage,
                mentions,
                unresolved_mentions,
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
                        mentions: appended.mentions.clone(),
                    });
                }
                EventPayload::ToolInvoked(invoked)
                    if &invoked.thread == source && current_turn <= limit =>
                {
                    plan.push(CopyItem::Tool {
                        original: invoked.tool_call.clone(),
                        tool: invoked.tool.clone(),
                        source: invoked.source.clone(),
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
                    mentions,
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
                            mentions,
                        }),
                        vec![fork.clone(), original],
                    ))?;
                }
                CopyItem::Tool {
                    original,
                    tool,
                    source,
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
                            source,
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

    fn build_request(
        &self,
        store: &GraphStore,
        thread: &SubjectRef,
        overrides: &TurnOverrides,
    ) -> ModelRequest {
        ModelRequest {
            model: overrides
                .model
                .clone()
                .unwrap_or_else(|| self.provider.descriptor().clone()),
            system: overrides.system.clone().or_else(|| self.system.clone()),
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
                        name: d.id.clone(),
                        description: d.description.clone(),
                        parameters: d.parameters,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Executes tool calls requested by the model: permission check via the
    /// registry, run, and record one durable `ToolInvoked` item per call.
    async fn run_tool_calls(
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
                    let result = registry
                        .invoke(store.graph(), &call.name, &call.arguments)
                        .await;
                    result.unwrap_or_else(|err| serde_json::json!({ "error": err.to_string() }))
                }
                None => serde_json::json!({ "error": "no tools registered" }),
            };
            let source = self
                .tools
                .as_ref()
                .and_then(|registry| registry.get(&call.name))
                .map(|entry| entry.descriptor.source.label());
            store.append(self.event(
                EventPayload::ToolInvoked(ToolInvoked {
                    thread: thread.clone(),
                    tool_call: tool_call.clone(),
                    tool: call.name.clone(),
                    source,
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
                Vec::new(),
            )?;
            outputs.push(output);
        }
        Ok(outputs)
    }

    /// Appends a message with its `@ns:kind:id` mentions (VIS-CHAT-004).
    /// Mention targets must already exist in the graph — callers resolve
    /// first via [`parse_mention_refs`] + [`Self::resolve_mentions`].
    fn append_message(
        &self,
        store: &mut GraphStore,
        thread: &SubjectRef,
        role: MessageRole,
        content: &str,
        turn: u64,
        mentions: Vec<SubjectRef>,
    ) -> Result<SubjectRef, ConversationError> {
        let message = SubjectRef::new(
            vistalith_domain::Namespace::Agentic,
            SubjectKind::Message,
            Uuid::now_v7().to_string(),
        )
        .expect("generated message id is valid");
        let mut subjects = vec![thread.clone(), message.clone()];
        subjects.extend(mentions.iter().cloned());
        store.append(self.event(
            EventPayload::MessageAppended(MessageAppended {
                thread: thread.clone(),
                message: message.clone(),
                role,
                content: content.to_owned(),
                turn,
                forked_of: None,
                mentions,
            }),
            subjects,
        ))?;
        Ok(message)
    }

    /// Filters parsed mention refs down to the ones that exist in the
    /// graph. Returns (resolved, unresolved raw strings).
    fn resolve_mentions(
        &self,
        store: &GraphStore,
        parsed: Vec<SubjectRef>,
    ) -> (Vec<SubjectRef>, Vec<String>) {
        let mut resolved = Vec::new();
        let mut unresolved = Vec::new();
        for reference in parsed {
            if store.graph().subject(&reference).is_some() {
                resolved.push(reference);
            } else {
                unresolved.push(reference.to_string());
            }
        }
        (resolved, unresolved)
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
        mentions: Vec<SubjectRef>,
    },
    Tool {
        original: SubjectRef,
        tool: String,
        source: Option<String>,
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
