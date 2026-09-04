use std::path::Path;

use serde::Serialize;
use uuid::Uuid;
use vistalith_domain::{Actor, EventPayload, PatchApplied, PatchRejected, StoredEvent, VEvent};

use crate::digest::{RawLog, StoredLog, StoredLogOut, graph_digest};
use crate::graph::SemanticWorldGraph;
use crate::patch::{GraphPatch, PatchOutcome, validate_patch};
use crate::projection::{ProjectionError, apply_event};

/// Errors that abort an append. The store is never left half-mutated: the
/// event is only committed once its projection has succeeded.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StoreError {
    #[error("duplicate event id {0}")]
    DuplicateEventId(Uuid),
    #[error(transparent)]
    Projection(#[from] ProjectionError),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("unknown revision {0}: the log only reaches revision {1}")]
    UnknownRevision(u64, u64),
    #[error(transparent)]
    BehaviorOutputRejected(#[from] crate::behavior::BehaviorError),
}

/// Log-assigned coordinates of a committed event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AppendedEvent {
    pub event_id: Uuid,
    pub sequence: u64,
    /// Graph revision produced by the event (equal to the previous revision
    /// when the event is not state-changing, e.g. a patch rejection).
    pub revision: u64,
}

/// The durable event log.
#[derive(Debug, Clone, Default)]
pub struct EventLog {
    entries: Vec<StoredEvent>,
    index: std::collections::HashMap<Uuid, usize>,
}

impl EventLog {
    pub fn entries(&self) -> &[StoredEvent] {
        &self.entries
    }

    pub fn next_sequence(&self) -> u64 {
        self.entries.len() as u64
    }

    fn contains(&self, event_id: Uuid) -> bool {
        self.index.contains_key(&event_id)
    }

    fn commit(&mut self, event: VEvent, revision: u64) -> AppendedEvent {
        let sequence = self.next_sequence();
        self.index.insert(event.event_id, self.entries.len());
        self.entries.push(StoredEvent {
            sequence,
            revision,
            event,
        });
        AppendedEvent {
            event_id: self.entries[sequence as usize].event.event_id,
            sequence,
            revision,
        }
    }
}

/// The append/project pipeline: commands become `VEvent`s, the log is the
/// durable source of truth, and the [`SemanticWorldGraph`] is the strict
/// projection of the log.
#[derive(Debug, Clone, Default)]
pub struct GraphStore {
    log: EventLog,
    graph: SemanticWorldGraph,
}

impl GraphStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends and projects one event. Idempotent on `event_id`: a duplicate
    /// is rejected and nothing changes.
    pub fn append(&mut self, event: VEvent) -> Result<AppendedEvent, StoreError> {
        if self.log.contains(event.event_id) {
            return Err(StoreError::DuplicateEventId(event.event_id));
        }
        let sequence = self.log.next_sequence();
        let revision = apply_event(&mut self.graph, &event, sequence)?;
        Ok(self.log.commit(event, revision))
    }

    /// Proposes a graph patch (SPEC-004). On success a `patch-applied` event
    /// is appended; on rejection a `patch-rejected` event is appended and the
    /// graph stays untouched. Both outcomes are durable (SPEC-002).
    pub fn propose_patch(&mut self, patch: GraphPatch) -> Result<PatchOutcome, StoreError> {
        let base_event = |payload| self.base_event(patch.proposed_by.clone(), payload);
        match validate_patch(&self.graph, &patch) {
            Ok(()) => {
                let event = base_event(EventPayload::PatchApplied(PatchApplied {
                    patch_id: patch.patch_id.clone(),
                    operations: patch.operations,
                }));
                let appended = self.append(event)?;
                Ok(PatchOutcome::Applied {
                    patch_id: patch.patch_id,
                    revision: appended.revision,
                })
            }
            Err(reason) => {
                let event = base_event(EventPayload::PatchRejected(PatchRejected {
                    patch_id: patch.patch_id.clone(),
                    reason: reason.to_string(),
                }));
                let appended = self.append(event)?;
                debug_assert_eq!(appended.revision, self.graph.revision());
                Ok(PatchOutcome::Rejected {
                    patch_id: patch.patch_id,
                    reason: reason.to_string(),
                })
            }
        }
    }

    fn base_event(&self, actor: Actor, payload: EventPayload) -> VEvent {
        VEvent {
            event_id: Uuid::now_v7(),
            actor,
            timestamp: time::OffsetDateTime::now_utc(),
            subjects: Vec::new(),
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            trace_id: None,
            payload,
        }
    }

    pub fn graph(&self) -> &SemanticWorldGraph {
        &self.graph
    }

    pub fn log(&self) -> &[StoredEvent] {
        self.log.entries()
    }

    /// SHA-256 fingerprint of the projected graph state.
    pub fn digest(&self) -> String {
        graph_digest(&self.graph)
    }

    /// Time travel: replays the durable log into a fresh projection stopped
    /// at `revision` (SPEC-011: "graph at event/revision"). Every stored
    /// entry carries the revision it produced, so the cut is exact even
    /// across revision-neutral events such as patch rejections.
    pub fn graph_at_revision(&self, revision: u64) -> Result<SemanticWorldGraph, StoreError> {
        let current = self.graph.revision();
        if revision > current {
            return Err(StoreError::UnknownRevision(revision, current));
        }
        let mut graph = SemanticWorldGraph::new();
        for stored in self.log.entries() {
            if stored.revision > revision {
                break;
            }
            crate::projection::apply_event(&mut graph, &stored.event, stored.sequence)?;
        }
        debug_assert_eq!(graph.revision(), revision);
        Ok(graph)
    }

    /// Structural diff between two revisions of this log (SPEC-011).
    pub fn diff_revisions(&self, from: u64, to: u64) -> Result<crate::diff::GraphDiff, StoreError> {
        let from_graph = self.graph_at_revision(from)?;
        let to_graph = self.graph_at_revision(to)?;
        Ok(crate::diff::diff_graphs(&from_graph, &to_graph))
    }

    /// Strict replay: rebuilds the store from raw events, assigning sequences
    /// and revisions in log order. Determinism: the same input always yields
    /// the same digest.
    pub fn replay(events: impl IntoIterator<Item = VEvent>) -> Result<Self, StoreError> {
        let mut store = GraphStore::new();
        for event in events {
            store.append(event)?;
        }
        Ok(store)
    }

    /// Rebuilds from stored (sequence/revision-carrying) log entries,
    /// verifying that the stored revision matches the replayed projection.
    pub fn rebuild(entries: impl IntoIterator<Item = StoredEvent>) -> Result<Self, StoreError> {
        let mut store = GraphStore::new();
        for stored in entries {
            let StoredEvent {
                sequence,
                revision,
                event,
            } = stored;
            let expected_sequence = store.log.next_sequence();
            if sequence != expected_sequence {
                return Err(StoreError::Serialization(format!(
                    "log gap: expected sequence {expected_sequence}, found {sequence}"
                )));
            }
            let appended = store.append(event)?;
            if appended.revision != revision {
                return Err(StoreError::Serialization(format!(
                    "revision mismatch at sequence {sequence}: log says {revision}, projection produced {}",
                    appended.revision
                )));
            }
        }
        Ok(store)
    }

    /// Serializes the durable log (sequence/revision included).
    pub fn to_log_json(&self) -> String {
        let out = StoredLogOut {
            events: self.log.entries(),
        };
        serde_json::to_string_pretty(&out).expect("stored log serialization cannot fail")
    }

    /// Loads a raw event log / fixture (`{"events": [ ... ]}`) from JSON.
    pub fn from_raw_json(json: &str) -> Result<Self, StoreError> {
        let raw: RawLog =
            serde_json::from_str(json).map_err(|e| StoreError::Serialization(e.to_string()))?;
        GraphStore::replay(raw.events)
    }

    /// Loads a stored log previously written by [`GraphStore::to_log_json`].
    pub fn from_stored_json(json: &str) -> Result<Self, StoreError> {
        let stored: StoredLog =
            serde_json::from_str(json).map_err(|e| StoreError::Serialization(e.to_string()))?;
        GraphStore::rebuild(stored.events)
    }

    /// Loads a fixture file from disk.
    pub fn from_fixture_path(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let json =
            std::fs::read_to_string(path).map_err(|e| StoreError::Serialization(e.to_string()))?;
        GraphStore::from_raw_json(&json)
    }
}
