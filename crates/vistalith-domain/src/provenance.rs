use std::fmt;

use serde::{Deserialize, Serialize};

/// Who or what produced a fact: `user:ruben`, `agent:<name>`,
/// `sddk:v1.82.0`, `system:vistalithd`, `fixture:sample-world`, ...
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Actor(String);

impl Actor {
    pub fn new(raw: impl Into<String>) -> Result<Self, crate::DomainError> {
        let raw = raw.into();
        if raw.is_empty() || raw.contains(char::is_whitespace) {
            return Err(crate::DomainError::InvalidActor(raw));
        }
        Ok(Actor(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a subject, relation or assertion came from and which revision
/// supports it (`GLOSSARY.md` / `graph/SEMANTIC-WORLD-GRAPH.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: Actor,
    /// Revision of the source the fact was observed at (e.g. the SDDK commit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Optional confidence in `0.0..=1.0` (derived/advisory facts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

impl Provenance {
    pub fn new(source: impl Into<String>) -> Result<Self, crate::DomainError> {
        Ok(Provenance {
            source: Actor::new(source)?,
            source_revision: None,
            note: None,
            confidence: None,
        })
    }

    pub fn with_source_revision(mut self, revision: impl Into<String>) -> Self {
        self.source_revision = Some(revision.into());
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Result<Self, crate::DomainError> {
        if !(0.0..=1.0).contains(&confidence) {
            return Err(crate::DomainError::InvalidConfidence(confidence));
        }
        self.confidence = Some(confidence);
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_actors() {
        assert!(Actor::new("user:ruben").is_ok());
        assert!(Actor::new("").is_err());
        assert!(Actor::new("has space").is_err());
    }

    #[test]
    fn confidence_bounds() {
        let p = Provenance::new("agent:claude").unwrap();
        assert!(p.clone().with_confidence(1.0).is_ok());
        assert!(p.clone().with_confidence(0.0).is_ok());
        assert!(p.with_confidence(1.5).is_err());
    }
}
