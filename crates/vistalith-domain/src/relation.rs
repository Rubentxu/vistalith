use std::fmt;

use serde::{Deserialize, Serialize};

use crate::authority::AuthorityClass;
use crate::provenance::Provenance;
use crate::subject::SubjectRef;

fn validate_snake(raw: &str) -> Result<(), crate::DomainError> {
    let valid = !raw.is_empty()
        && raw.starts_with(|c: char| c.is_ascii_lowercase())
        && raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if valid {
        Ok(())
    } else {
        Err(crate::DomainError::InvalidRelationKind(raw.to_owned()))
    }
}

/// Typed relation kind: the edge families from
/// `graph/SEMANTIC-WORLD-GRAPH.md`, plus a validated catch-all so new
/// relation types can be used before the vocabulary is extended.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelationKind {
    Contains,
    DependsOn,
    Calls,
    Implements,
    Exposes,
    Satisfies,
    Verifies,
    TestedBy,
    DecidedBy,
    MotivatedBy,
    ProducedBy,
    ExecutedBy,
    DelegatedTo,
    Affects,
    Blocks,
    DerivesFrom,
    Supersedes,
    Contradicts,
    Supports,
    ProvidesEvidenceFor,
    Visualizes,
    Mentions,
    ProposesChangeTo,
    RejectedInFavorOf,
    Revisits,
    ObservedIn,
    UsedModel,
    UsedTool,
    /// An exploration fork derived from this thread/graph state (SPEC-011).
    ForkedFrom,
    /// An agent run contributing to a frame/subject (AGENTS-DELEGATION.md).
    ContributesTo,
    /// A relation kind outside the shipped vocabulary (snake_case).
    Other(String),
}

macro_rules! relation_kind_impl {
    ($($variant:ident => $snake:literal),+ $(,)?) => {
        impl RelationKind {
            pub fn as_str(&self) -> &str {
                match self {
                    $(RelationKind::$variant => $snake,)+
                    RelationKind::Other(raw) => raw,
                }
            }

            pub fn parse(raw: &str) -> Result<Self, crate::DomainError> {
                match raw {
                    $($snake => Ok(RelationKind::$variant),)+
                    _ => {
                        validate_snake(raw)?;
                        Ok(RelationKind::Other(raw.to_owned()))
                    }
                }
            }
        }

        impl fmt::Display for RelationKind {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for RelationKind {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for RelationKind {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                RelationKind::parse(&raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

relation_kind_impl! {
    Contains => "contains",
    DependsOn => "depends_on",
    Calls => "calls",
    Implements => "implements",
    Exposes => "exposes",
    Satisfies => "satisfies",
    Verifies => "verifies",
    TestedBy => "tested_by",
    DecidedBy => "decided_by",
    MotivatedBy => "motivated_by",
    ProducedBy => "produced_by",
    ExecutedBy => "executed_by",
    DelegatedTo => "delegated_to",
    Affects => "affects",
    Blocks => "blocks",
    DerivesFrom => "derives_from",
    Supersedes => "supersedes",
    Contradicts => "contradicts",
    Supports => "supports",
    ProvidesEvidenceFor => "provides_evidence_for",
    Visualizes => "visualizes",
    Mentions => "mentions",
    ProposesChangeTo => "proposes_change_to",
    RejectedInFavorOf => "rejected_in_favor_of",
    Revisits => "revisits",
    ObservedIn => "observed_in",
    UsedModel => "used_model",
    UsedTool => "used_tool",
    ForkedFrom => "forked_from",
    ContributesTo => "contributes_to",
}

/// Directed typed relation between two subjects.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RelationRef {
    pub from: SubjectRef,
    pub kind: RelationKind,
    pub to: SubjectRef,
}

impl RelationRef {
    pub fn new(
        from: SubjectRef,
        kind: RelationKind,
        to: SubjectRef,
    ) -> Result<Self, crate::DomainError> {
        if from == to {
            return Err(crate::DomainError::SelfRelation(Box::new(Self {
                from,
                kind,
                to,
            })));
        }
        Ok(RelationRef { from, kind, to })
    }
}

impl fmt::Display for RelationRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({})-{}->({})", self.from, self.kind, self.to)
    }
}

/// A graph fact: relation identity plus its authority and provenance
/// (every graph fact carries them — `graph/SEMANTIC-WORLD-GRAPH.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationFact {
    pub relation: RelationRef,
    pub authority: AuthorityClass,
    pub provenance: Provenance,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subject::{Namespace, SubjectKind};

    fn subject(id: &str) -> SubjectRef {
        SubjectRef::new(Namespace::Arch, SubjectKind::Container, id).unwrap()
    }

    #[test]
    fn rejects_self_relations() {
        let s = subject("payment-service");
        assert!(RelationRef::new(s.clone(), RelationKind::DependsOn, s).is_err());
    }

    #[test]
    fn parses_and_validates_kinds() {
        assert_eq!(
            RelationKind::parse("depends_on").unwrap(),
            RelationKind::DependsOn
        );
        assert_eq!(RelationKind::Verifies.as_str(), "verifies");
        assert!(RelationKind::parse("Depends-On").is_err());
        // Unknown but well-formed kinds stay usable.
        assert!(matches!(
            RelationKind::parse("tracks_cost_for").unwrap(),
            RelationKind::Other(_)
        ));
        // Serde keeps the vocabulary typed.
        let back: RelationKind =
            serde_json::from_str("\"implements\"").expect("typed relation kind");
        assert_eq!(back, RelationKind::Implements);
    }
}
