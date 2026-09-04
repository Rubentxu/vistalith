use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::authority::AuthorityClass;
use crate::provenance::Provenance;
use crate::relation::RelationFact;
use crate::subject::SubjectRef;

/// Identifier of a graph patch proposal (SPEC-004: proposed → applied|rejected).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PatchId(String);

impl PatchId {
    pub fn new(raw: impl Into<String>) -> Result<Self, crate::DomainError> {
        let raw = raw.into();
        if raw.is_empty() || raw.contains(char::is_whitespace) {
            return Err(crate::DomainError::InvalidPatchId(raw));
        }
        Ok(PatchId(raw))
    }

    pub fn generate() -> Self {
        PatchId(format!("patch-{}", Uuid::now_v7()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PatchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One mutation step inside a graph patch.
///
/// All operations are Vistalith-graph mutations. When an operation would
/// authoritatively mutate an SDDK-owned subject, the whole patch is rejected
/// and must be converted into a governed SDDK semantic proposal instead
/// (SPEC-001/SPEC-004); the policy is enforced by `vistalith-graph`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum PatchOperation {
    /// Inserts a subject or merges properties into an existing one.
    UpsertSubject {
        subject: SubjectRef,
        authority: AuthorityClass,
        provenance: Provenance,
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        properties: std::collections::BTreeMap<String, serde_json::Value>,
    },
    /// Declares a typed relation; endpoints must already exist.
    DeclareRelation { fact: RelationFact },
    /// Marks a subject deprecated; it stays in the graph, distinguishable.
    DeprecateSubject {
        subject: SubjectRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}
