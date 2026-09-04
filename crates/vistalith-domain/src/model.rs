use serde::{Deserialize, Serialize};

/// Conversation message roles (SPEC-007: Thread/Turn/Item is durable
/// Vistalith state; typed items are never flattened to prose — tool and
/// delegation items keep their own role).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

/// Vistalith-owned model identity (SPEC-008: provider/model types are
/// Vistalith-owned; Rig stays an adapter and never leaks its types here).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelDescriptor {
    /// Provider id, e.g. `anthropic`, `fake`.
    pub provider: String,
    /// Model id at the provider, e.g. `claude-haiku-4-5`.
    pub model: String,
}

impl ModelDescriptor {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        ModelDescriptor {
            provider: provider.into(),
            model: model.into(),
        }
    }

    /// Subject id for this model in the SWG (`agentic:model:<provider>/<model>`).
    pub fn subject_id(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

impl std::fmt::Display for ModelDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.provider, self.model)
    }
}

/// Token usage reported for one model call. Provider-agnostic on purpose:
/// adapters normalize their wire formats into this shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_subject_id_is_a_valid_subject_id() {
        let descriptor = ModelDescriptor::new("anthropic", "claude-haiku-4-5");
        assert_eq!(descriptor.subject_id(), "anthropic/claude-haiku-4-5");
        // '/' is allowed in subject ids (only ':' and '@' are reserved).
        crate::SubjectRef::parse("agentic:model:anthropic/claude-haiku-4-5")
            .expect("model subject id parses");
    }

    #[test]
    fn roles_roundtrip_serde() {
        let role: MessageRole = serde_json::from_str("\"assistant\"").unwrap();
        assert_eq!(role, MessageRole::Assistant);
        assert_eq!(
            serde_json::to_string(&MessageRole::Tool).unwrap(),
            "\"tool\""
        );
    }
}
