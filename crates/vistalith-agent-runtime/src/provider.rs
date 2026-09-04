//! Vistalith-owned provider contracts (SPEC-008, `agentic/RIG-STRATEGY.md`).
//!
//! Rig is an adapter: only [`RigProvider`] touches rig types, and none of them
//! appear in this module's signatures.

use vistalith_domain::{MessageRole, ModelDescriptor, ModelUsage};

use thiserror::Error;

/// One normalized chat message handed to (or returned from) a provider.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

/// A Vistalith-owned completion request.
#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub model: ModelDescriptor,
    /// System instructions, sent before the history.
    pub system: Option<String>,
    /// Conversation history in chronological order; the last message is the
    /// current prompt.
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u64>,
}

/// A Vistalith-owned completion response.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelResponse {
    pub content: String,
    pub model: ModelDescriptor,
    pub usage: ModelUsage,
}

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("provider `{0}` failed: {1}")]
    ProviderFailed(String, String),
    #[error("provider returned an empty response")]
    EmptyResponse,
}

/// The Vistalith provider contract. Implementations are the only place where
/// provider SDKs are allowed to exist.
pub trait ModelProvider {
    fn descriptor(&self) -> &ModelDescriptor;

    fn complete(
        &self,
        request: ModelRequest,
    ) -> impl std::future::Future<Output = Result<ModelResponse, ModelError>> + Send;
}

/// Shared providers stay providers: `Arc<T>` forwards to `T`.
impl<T> ModelProvider for std::sync::Arc<T>
where
    T: ModelProvider + Send + Sync + ?Sized,
{
    fn descriptor(&self) -> &ModelDescriptor {
        (**self).descriptor()
    }

    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        (**self).complete(request).await
    }
}

/// Offline provider with scripted replies (`DeterminismClass::RecordedExternal`):
/// replies pop in order and the last one repeats. Usage numbers are fixed so
/// tests and offline demos stay deterministic.
pub struct FakeProvider {
    model: ModelDescriptor,
    replies: std::sync::Mutex<Vec<String>>,
    requests: std::sync::Mutex<Vec<ModelRequest>>,
}

impl FakeProvider {
    /// Repeats `reply` forever.
    pub fn repeating(reply: impl Into<String>) -> Self {
        FakeProvider {
            model: ModelDescriptor::new("fake", "echo-1"),
            replies: std::sync::Mutex::new(vec![reply.into()]),
            requests: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Pops scripted replies in order, repeating the last one when exhausted.
    pub fn scripted(replies: Vec<String>) -> Self {
        FakeProvider {
            model: ModelDescriptor::new("fake", "scripted-1"),
            replies: std::sync::Mutex::new(replies),
            requests: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Requests this provider received, for tests that assert on the
    /// reconstructed conversation history.
    pub fn recorded_requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().expect("requests lock").clone()
    }
}

impl ModelProvider for FakeProvider {
    fn descriptor(&self) -> &ModelDescriptor {
        &self.model
    }

    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        self.requests
            .lock()
            .expect("requests lock")
            .push(request.clone());
        let mut replies = self.replies.lock().expect("scripted replies lock");
        if replies.is_empty() {
            return Err(ModelError::EmptyResponse);
        }
        let content = replies[0].clone();
        if replies.len() > 1 {
            replies.remove(0);
        }
        Ok(ModelResponse {
            content,
            model: self.model.clone(),
            usage: ModelUsage {
                input_tokens: estimate_tokens(&request.messages),
                output_tokens: 8,
                total_tokens: 8 + estimate_tokens(&request.messages),
            },
        })
    }
}

fn estimate_tokens(messages: &[ChatMessage]) -> u64 {
    messages
        .iter()
        .map(|m| (m.content.len() as u64).div_ceil(4))
        .sum()
}

/// The runtime the server speaks to: one of the built-in providers, enum
/// dispatched (no plugin system before a measured need — B10).
pub enum RuntimeProvider {
    Fake(FakeProvider),
    Rig(RigProvider),
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error("rig provider requires an API key (set VISTALITH_ANTHROPIC_API_KEY)")]
    MissingApiKey,
}

impl RuntimeProvider {
    pub fn descriptor(&self) -> &ModelDescriptor {
        match self {
            RuntimeProvider::Fake(provider) => provider.descriptor(),
            RuntimeProvider::Rig(provider) => provider.descriptor(),
        }
    }

    pub async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, RuntimeError> {
        match self {
            RuntimeProvider::Fake(provider) => Ok(provider.complete(request).await?),
            RuntimeProvider::Rig(provider) => Ok(provider.complete(request).await?),
        }
    }
}

impl ModelProvider for RuntimeProvider {
    fn descriptor(&self) -> &ModelDescriptor {
        RuntimeProvider::descriptor(self)
    }

    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        RuntimeProvider::complete(self, request)
            .await
            .map_err(|e| match e {
                RuntimeError::Model(model_error) => model_error,
                other => ModelError::ProviderFailed("runtime".into(), other.to_string()),
            })
    }
}

/// Rig-backed provider (ADR-008): the only place where rig-core types exist.
/// Currently Anthropic; further providers join as new constructors.
pub struct RigProvider {
    model: ModelDescriptor,
    inner: rig_core::providers::anthropic::completion::GenericCompletionModel,
}

impl RigProvider {
    /// Builds an Anthropic-backed provider. `api_key` must be present; callers
    /// own credential storage and MUST NOT log or return it (SPEC-008).
    pub fn anthropic(
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, RuntimeError> {
        use rig_core::client::completion::CompletionClient;

        let model_name = model.into();
        let client = rig_core::providers::anthropic::Client::builder()
            .api_key(api_key.into())
            .build()
            .map_err(|e| ModelError::ProviderFailed("anthropic".into(), e.to_string()))?;
        let inner = client.completion_model(model_name.clone());
        Ok(RigProvider {
            model: ModelDescriptor::new("anthropic", model_name),
            inner,
        })
    }
}

impl ModelProvider for RigProvider {
    fn descriptor(&self) -> &ModelDescriptor {
        &self.model
    }

    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        use rig_core::completion::CompletionModel;

        let mut history = Vec::with_capacity(request.messages.len() + 1);
        if let Some(system) = &request.system {
            history.push(rig_core::completion::Message::system(system.clone()));
        }
        for message in &request.messages {
            history.push(match message.role {
                MessageRole::User | MessageRole::Tool => {
                    rig_core::completion::Message::user(message.content.clone())
                }
                MessageRole::Assistant | MessageRole::System => {
                    rig_core::completion::Message::assistant(message.content.clone())
                }
            });
        }

        let completion_request = rig_core::completion::CompletionRequest {
            model: Some(self.model.model.clone()),
            preamble: None,
            chat_history: history,
            documents: Vec::new(),
            tools: Vec::new(),
            temperature: None,
            max_tokens: request.max_tokens,
            tool_choice: None,
            additional_params: None,
            output_schema: None,
            record_telemetry_content: false,
        };

        let response = self
            .inner
            .completion(completion_request)
            .await
            .map_err(|e| ModelError::ProviderFailed(self.model.provider.clone(), e.to_string()))?;

        let content = response
            .choice
            .iter()
            .filter_map(|part| match part {
                rig_core::completion::AssistantContent::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        if content.is_empty() {
            return Err(ModelError::EmptyResponse);
        }

        Ok(ModelResponse {
            content,
            model: self.model.clone(),
            usage: ModelUsage {
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
                total_tokens: response.usage.total_tokens,
            },
        })
    }
}
