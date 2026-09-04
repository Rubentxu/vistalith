use serde::{Deserialize, Serialize};

/// Authority class of a graph fact (`GLOSSARY.md`).
///
/// Invariants (SPEC-001): advisory facts are visually distinguishable and
/// SDDK-owned subjects cannot be authoritatively mutated through a Vistalith
/// graph patch — enforcement lives in `vistalith-graph`, classification here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthorityClass {
    /// Fact owned by its source of truth (e.g. SDDK for work items, the
    /// human for architecture models authored directly in Vistalith).
    Authoritative,
    /// Computed/observed from another authoritative fact; carries provenance.
    Derived,
    /// Suggestion or assertion without authoritative backing.
    Advisory,
    /// Only meaningful within a live session; never durable.
    Ephemeral,
}

impl AuthorityClass {
    pub fn is_authoritative(self) -> bool {
        matches!(self, AuthorityClass::Authoritative)
    }
}
