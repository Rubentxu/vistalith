//! Vistalith domain value types.
//!
//! Slice-1 scope (`roadmap/IMPLEMENT-NOW.md`): stable `SubjectRef`s (ADR-011),
//! durable `VEvent`s (SPEC-002), graph patch value types (SPEC-004) and the
//! determinism classes that make fixture replay well-defined.
//!
//! SDDK crates are direct dependencies (ADR-001): helpers here translate
//! SDDK identities into observed `SubjectRef`s without any façade.

mod authority;
mod error;
mod event;
mod model;
mod patch;
mod provenance;
mod relation;
mod subject;

pub use authority::AuthorityClass;
pub use error::DomainError;
pub use event::{
    AdvisoryRaised, AgentDefined, CanvasBound, CanvasGeometry, DeterminismClass, EventPayload,
    FrameClosed, FrameOutcome,
    AgentRunFinished, FrameStarted, FrameTurnCompleted, UatCheckRecorded, UatVerdict, IntentDrafted, IntentOutcome, IntentPromoted,
    MessageAppended, PatchApplied, PatchRejected, RelationDeclared, StoredEvent, SubjectDefined,
    SddkProposalSubmitted, SubjectDeprecated, SubjectUpdated, ThreadForked, ThreadStarted, ToolInvoked, TurnCompleted,
    VEvent,
};
pub use model::{MessageRole, ModelDescriptor, ModelUsage};
pub use patch::{PatchId, PatchOperation};
pub use provenance::{Actor, Provenance};
pub use relation::{RelationFact, RelationKind, RelationRef};
pub use subject::{Namespace, SubjectKind, SubjectRef};
