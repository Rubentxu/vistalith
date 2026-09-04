use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("invalid namespace: `{0}`")]
    InvalidNamespace(String),
    #[error("invalid subject kind: `{0}`")]
    InvalidKind(String),
    #[error("invalid subject id: `{0}` (empty, or contains ':' or '@')")]
    InvalidSubjectId(String),
    #[error("invalid subject reference: `{0}` (expected namespace:kind:id[@revision])")]
    InvalidSubjectRef(String),
    #[error("invalid relation kind: `{0}`")]
    InvalidRelationKind(String),
    #[error("self relations are not allowed: {0}")]
    SelfRelation(Box<crate::relation::RelationRef>),
    #[error("invalid actor: `{0}`")]
    InvalidActor(String),
    #[error("invalid patch id: `{0}`")]
    InvalidPatchId(String),
    #[error("confidence must be within 0.0..=1.0, got {0}")]
    InvalidConfidence(f32),
}
