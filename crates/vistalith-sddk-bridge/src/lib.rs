//! Governed SDDK promotion bridge (SPK-012, milestone M7).
//!
//! B1 says SDDK is the core; when a Vistalith intent targets an SDDK-owned
//! subject, the change must go through SDDK's own decision plane — never
//! around it. This crate is that bridge, using the pinned SDDK crates
//! directly (no façade): the intent becomes a `sddk_domain::Proposal`, the
//! SDDK `CapabilityGateway` evaluates its default-deny policy from the
//! project's workflow manifest, and an allowed proposal executes the
//! evidence-bundle capability and persists a receipt in the SDDK ledger.
//!
//! Vistalith then records the whole thing — decision plus receipt — as a
//! durable `sddk-proposal-submitted` event projected into the SWG as a
//! *derived* observation with a `provides_evidence_for` edge to the target.
//! Vistalith never writes SDDK state itself; it proposes, and SDDK decides.

pub mod pull_up;

use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use vistalith_domain::{
    Actor, EventPayload, Namespace, SddkProposalSubmitted, SubjectKind, SubjectRef, VEvent,
};
use vistalith_graph::GraphStore;

pub use pull_up::{FocusAnswer, FocusTest, PullUpClassification, PullUpError, PullUpEvaluation, PullUpOutcome};

use sddk_domain::proposal::Proposal;
use sddk_domain::workflow::WorkflowManifest;
use sddk_gateway::{CapabilityGateway, GatewayError};
use sddk_storage::Storage;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("cannot open SDDK ledger at {0}: {1}")]
    Ledger(std::path::PathBuf, String),
    #[error("cannot load SDDK workflow manifest from {0}: {1}")]
    Workflow(std::path::PathBuf, String),
    #[error("SDDK gateway rejected the proposal: {0}")]
    Gateway(#[from] GatewayError),
    #[error("serialization error: {0}")]
    Serialization(String),
}

/// How the SDDK decision plane answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProposalDecision {
    /// Policy allowed the capability; the receipt is durable in the ledger.
    Allowed,
    /// The capability is not declared in the workflow (default-deny).
    Denied,
    /// The capability requires human approval which was not supplied.
    ApprovalRequired,
}

impl ProposalDecision {
    pub fn wire(&self) -> &'static str {
        match self {
            ProposalDecision::Allowed => "allowed",
            ProposalDecision::Denied => "denied",
            ProposalDecision::ApprovalRequired => "approval-required",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProposalOutcome {
    pub decision: ProposalDecision,
    /// SDDK receipt id when the capability executed.
    pub receipt_id: Option<String>,
    /// Full SDDK receipt (or the policy payload) as JSON.
    pub receipt: serde_json::Value,
}

/// The bridge over one SDDK project: workflow manifest + ledger.
pub struct SddkBridge {
    gateway: std::sync::Mutex<CapabilityGateway>,
    project_id: String,
    workflow_path: std::path::PathBuf,
    ledger_path: std::path::PathBuf,
}

impl SddkBridge {
    /// Opens (or creates) the SDDK ledger and loads the workflow manifest.
    pub fn open(
        ledger_path: impl AsRef<std::path::Path>,
        workflow_path: impl AsRef<std::path::Path>,
        project_id: impl Into<String>,
    ) -> Result<Self, BridgeError> {
        let workflow_path = workflow_path.as_ref().to_path_buf();
        let raw = std::fs::read_to_string(&workflow_path).map_err(|e| {
            BridgeError::Workflow(workflow_path.clone(), e.to_string())
        })?;
        let workflow: WorkflowManifest = serde_json::from_str(&raw)
            .map_err(|e| BridgeError::Workflow(workflow_path.clone(), e.to_string()))?;
        let storage = Storage::open(ledger_path.as_ref())
            .map_err(|e| BridgeError::Ledger(ledger_path.as_ref().to_path_buf(), e.to_string()))?;
        let project_id = project_id.into();
        Self::ensure_project(&storage, &project_id)?;
        Ok(SddkBridge {
            gateway: std::sync::Mutex::new(CapabilityGateway::new(
                sddk_gateway::CapabilityPolicy::from_workflow(&workflow),
                workflow,
                storage,
            )),
            project_id,
            workflow_path,
            ledger_path: ledger_path.as_ref().to_path_buf(),
        })
    }

    /// The receipt ledger has a foreign key on the project: make sure the
    /// bridged project exists (idempotent).
    fn ensure_project(storage: &Storage, project_id: &str) -> Result<(), BridgeError> {
        let exists = storage
            .get_project_optional(project_id)
            .map_err(|e| BridgeError::Ledger(std::path::PathBuf::from(project_id), e.to_string()))?;
        if exists.is_some() {
            return Ok(());
        }
        let created = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        storage
            .insert_project(&sddk_domain::ProjectRecord {
                project_id: project_id.to_owned(),
                display_name: format!("Vistalith bridge for {project_id}"),
                remote_url: None,
                scope: "vistalith-bridge".to_owned(),
                created_at: created,
            })
            .map_err(|e| BridgeError::Ledger(std::path::PathBuf::from(project_id), e.to_string()))
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn workflow_path(&self) -> &std::path::Path {
        &self.workflow_path
    }

    /// Submits a governed evidence proposal for `target` and appends the
    /// durable `sddk-proposal-submitted` event to the Vistalith log. The
    /// decision — allowed, denied or approval-required — is always durable;
    /// an allowed proposal carries the SDDK receipt.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_evidence_proposal(
        &self,
        store: &mut GraphStore,
        intent: &SubjectRef,
        target: &SubjectRef,
        bundle: serde_json::Value,
        reason: impl Into<String>,
        actor: &Actor,
        approve: bool,
    ) -> Result<SubjectRef, BridgeError> {
        let reason = reason.into();
        let proposal_outcome = self.execute(intent, target, bundle, reason, actor, approve)?;

        let proposal = SubjectRef::new(
            Namespace::Vistalith,
            SubjectKind::Proposal,
            Uuid::now_v7().to_string(),
        )
        .expect("generated proposal id is valid");
        store.append(event(
            actor.clone(),
            EventPayload::SddkProposalSubmitted(SddkProposalSubmitted {
                proposal: proposal.clone(),
                intent: intent.clone(),
                target: target.clone(),
                capability: "evidence.write".to_owned(),
                decision: proposal_outcome.decision.wire().to_owned(),
                receipt_id: proposal_outcome.receipt_id.clone(),
                receipt: proposal_outcome.receipt,
            }),
            vec![proposal.clone(), intent.clone(), target.clone()],
        ))
        .map_err(|e| BridgeError::Serialization(e.to_string()))?;
        Ok(proposal)
    }

    /// Runs the governed chain in SDDK: policy -> evidence capability ->
    /// receipt. Denied and approval-required proposals never execute.
    fn execute(
        &self,
        intent: &SubjectRef,
        _target: &SubjectRef,
        bundle: serde_json::Value,
        reason: String,
        actor: &Actor,
        approve: bool,
    ) -> Result<ProposalOutcome, BridgeError> {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let expires = (time::OffsetDateTime::now_utc() + time::Duration::hours(1))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let proposal = Proposal {
            proposal_id: Uuid::now_v7().to_string(),
            project_id: self.project_id.clone(),
            cycle_id: None,
            reason,
            capability: "evidence.write".to_owned(),
            // The typed runner takes the evidence bundle as its first
            // argument (EvidenceBundleWriteCapability contract).
            program: "vistalith-bridge".to_owned(),
            args: vec![serde_json::to_string(&bundle)
                .map_err(|e| BridgeError::Serialization(e.to_string()))?],
            env: std::collections::BTreeMap::new(),
            timeout_ms: 5_000,
            output_max_bytes: 64 * 1024,
            created_at: now,
            expires_at: expires,
            agent_version_hash: agent_version_hash(actor),
            behavior_version_hash: behavior_version_hash(intent),
            status: sddk_domain::proposal::ProposalStatus::Pending,
        };

        let mut gateway = self.gateway.lock().expect("sddk gateway lock");
        match gateway.execute_governed(proposal, approve) {
            Ok(receipt) => Ok(ProposalOutcome {
                decision: ProposalDecision::Allowed,
                receipt_id: Some(receipt.receipt_id.clone()),
                receipt: receipt_json(&receipt),
            }),
            Err(GatewayError::Denied { .. }) => Ok(ProposalOutcome {
                decision: ProposalDecision::Denied,
                receipt_id: None,
                receipt: serde_json::json!({ "decision": "denied" }),
            }),
            Err(GatewayError::ApprovalRequired { .. }) => Ok(ProposalOutcome {
                decision: ProposalDecision::ApprovalRequired,
                receipt_id: None,
                receipt: serde_json::json!({ "decision": "approval-required" }),
            }),
            Err(err) => Err(err.into()),
        }
    }

    /// Projects the SDDK ledger into the SWG (milestone M6): every cycle
    /// observed in the ledger becomes a derived `sddk:workflow:<cycle>`
    /// subject — the same `SubjectRef` identity the C4 and chat lenses use —
    /// with status/phase/branch properties from the latest ledger event, and
    /// a `derives_from` link to the observed project.
    ///
    /// Idempotent by construction: event ids are derived from the ledger
    /// sequence they materialize (`sync-<seq>-<event_type>`), so re-running
    /// a sync that observes no new events appends nothing; a refresh of an
    /// existing cycle emits a `subject-updated` with fresh properties.
    pub fn sync_workflow(
        &self,
        store: &mut GraphStore,
        actor: &Actor,
    ) -> Result<SyncReport, BridgeError> {
        let ledger_events = {
            let storage = Storage::open(self.ledger_path())
                .map_err(|e| BridgeError::Ledger(self.ledger_path().to_path_buf(), e.to_string()))?;
            storage
                .load_all_ledger_events()
                .map_err(|e| BridgeError::Ledger(self.ledger_path().to_path_buf(), e.to_string()))?
        };

        let mut report = SyncReport::default();
        let project_subject =
            SubjectRef::observed_sddk_project(&sddk_domain::ProjectId::new(self.project_id.clone())
                .map_err(|e| BridgeError::Serialization(e.to_string()))?);

        // The observed project subject itself.
        if store.graph().subject(&project_subject).is_none() {
            store
                .append(defined_event(
                    actor,
                    project_subject.clone(),
                    serde_json::json!({
                        "project_id": self.project_id,
                        "source": "sddk-ledger-sync",
                    }),
                ))
                .map_err(|e| BridgeError::Serialization(e.to_string()))?;
            report.subjects_created += 1;
        }

        // Latest event per cycle, in ledger order (later wins).
        let mut latest: std::collections::BTreeMap<String, &sddk_domain::LedgerEvent> =
            std::collections::BTreeMap::new();
        for event in &ledger_events {
            if let Some(cycle_id) = &event.cycle_id {
                latest.insert(cycle_id.clone(), event);
            }
        }

        for (cycle_id, ledger_event) in &latest {
            let ledger = ledger_event;
            let cycle_subject = SubjectRef::new(
                Namespace::Sddk,
                SubjectKind::Workflow,
                cycle_id.clone(),
            )
            .map_err(|e| BridgeError::Serialization(e.to_string()))?;

            // Parse the post-state defensively: the ledger stores whatever
            // the engine committed (cycle manifest snapshots).
            let state = ledger
                .state_after
                .clone()
                .unwrap_or_else(|| ledger.state_before.clone().unwrap_or(serde_json::Value::Null));
            let pick = |key: &str| state.get(key).cloned().unwrap_or(serde_json::Value::Null);

            let mut properties = std::collections::BTreeMap::new();
            properties.insert("cycle_id".to_owned(), serde_json::json!(cycle_id));
            properties.insert("project_id".to_owned(), serde_json::json!(self.project_id));
            for key in ["display_name", "status", "phase", "path", "branch"] {
                let value = pick(key);
                if !value.is_null() {
                    properties.insert(key.to_owned(), value);
                }
            }
            properties.insert(
                "ledger_sequence".to_owned(),
                serde_json::json!(ledger.sequence),
            );
            properties.insert(
                "last_event_type".to_owned(),
                serde_json::json!(ledger.event_type),
            );

            let exists = store.graph().subject(&cycle_subject).is_some();
            let payload = if exists {
                EventPayload::SubjectUpdated(vistalith_domain::SubjectUpdated {
                    subject: cycle_subject.clone(),
                    properties: properties.clone(),
                })
            } else {
                EventPayload::SubjectDefined(vistalith_domain::SubjectDefined {
                    subject: cycle_subject.clone(),
                    authority: vistalith_domain::AuthorityClass::Derived,
                    provenance: vistalith_domain::Provenance {
                        source: actor.clone(),
                        source_revision: Some(ledger.sequence.to_string()),
                        note: Some("projected from the SDDK ledger".to_owned()),
                        confidence: None,
                    },
                    properties: properties.clone(),
                })
            };
            // Deterministic event id: the same ledger state always maps to
            // the same event, so re-syncing is a no-op (duplicate rejected).
            let mut sync_event =
                event(actor.clone(), payload, vec![cycle_subject.clone()]);
            sync_event.event_id = deterministic_event_id(&[
                "sddk-sync",
                &self.project_id,
                cycle_id,
                &ledger.sequence.to_string(),
            ]);
            // ...where event.sequence is the ledger sequence of the trigger.
            match store.append(sync_event) {
                Ok(_) => {
                    if exists {
                        report.subjects_updated += 1;
                    } else {
                        report.subjects_created += 1;
                    }
                }
                Err(vistalith_graph::StoreError::DuplicateEventId(_)) => {
                    report.events_skipped += 1;
                }
                Err(err) => return Err(BridgeError::Serialization(err.to_string())),
            }

            // Cycle derives from the observed project.
            if !store.graph().relations().any(|fact| {
                fact.relation.from == cycle_subject
                    && fact.relation.to == project_subject
                    && fact.relation.kind == vistalith_domain::RelationKind::DerivesFrom
            }) {
                store
                    .append(event(
                        actor.clone(),
                        EventPayload::RelationDeclared(vistalith_domain::RelationDeclared {
                            fact: vistalith_domain::RelationFact {
                                relation: vistalith_domain::RelationRef::new(
                                    cycle_subject.clone(),
                                    vistalith_domain::RelationKind::DerivesFrom,
                                    project_subject.clone(),
                                )
                                .expect("distinct sddk endpoints"),
                                authority: vistalith_domain::AuthorityClass::Derived,
                                provenance: vistalith_domain::Provenance::new(
                                    "system:sddk-sync",
                                )
                                .expect("static provenance"),
                            },
                        }),
                        vec![cycle_subject.clone(), project_subject.clone()],
                    ))
                    .map_err(|e| BridgeError::Serialization(e.to_string()))?;
                report.relations_declared += 1;
            }
        }

        Ok(report)
    }

    fn ledger_path(&self) -> &std::path::Path {
        &self.ledger_path
    }

    /// Lists the SDDK ledger's receipts for the bridged project.
    pub fn receipts(&self) -> Result<Vec<serde_json::Value>, BridgeError> {
        let gateway = self.gateway.lock().expect("sddk gateway lock");
        let receipts = gateway.receipts(&self.project_id)?;
        Ok(receipts.iter().map(receipt_json).collect())
    }
}

fn receipt_json(receipt: &sddk_domain::CapabilityReceipt) -> serde_json::Value {
    serde_json::json!({
        "receipt_id": receipt.receipt_id,
        "project_id": receipt.project_id,
        "cycle_id": receipt.cycle_id,
        "capability": receipt.capability,
        "request_hash": receipt.request_hash,
        "status": receipt.status,
        "result": receipt.result,
        "started_at": receipt.started_at,
        "completed_at": receipt.completed_at,
        "agent_version_hash": receipt.agent_version_hash,
        "behavior_version_hash": receipt.behavior_version_hash,
    })
}

/// Deterministic agent identity hash (SDDK policy requires non-empty
/// version hashes: who is proposing).
fn agent_version_hash(actor: &Actor) -> String {
    let mut hasher = Sha256::new();
    hasher.update(concat!(env!("CARGO_PKG_VERSION"), ":").as_bytes());
    hasher.update(actor.as_str().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Deterministic behavior identity hash (what authorized the proposal: the
/// Vistalith intent that carries the change).
fn behavior_version_hash(intent: &SubjectRef) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"vistalith:intent:");
    hasher.update(intent.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SyncReport {
    pub subjects_created: u64,
    pub subjects_updated: u64,
    pub relations_declared: u64,
    pub events_skipped: u64,
}

fn defined_event(actor: &Actor, subject: SubjectRef, properties: serde_json::Value) -> VEvent {
    let properties: std::collections::BTreeMap<String, serde_json::Value> = serde_json::from_value(
        properties,
    )
    .expect("object properties");
    event(
        actor.clone(),
        EventPayload::SubjectDefined(vistalith_domain::SubjectDefined {
            subject,
            authority: vistalith_domain::AuthorityClass::Derived,
            provenance: vistalith_domain::Provenance::new("system:sddk-sync")
                .expect("static provenance"),
            properties,
        }),
        Vec::new(),
    )
}

/// UUIDv5-style deterministic id (SHA-256 truncated, version/variant bits
/// set) so identical sync inputs map to identical events.
fn deterministic_event_id(parts: &[&str]) -> uuid::Uuid {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let digest: [u8; 16] = hasher.finalize()[..16]
        .try_into()
        .expect("16-byte digest prefix");
    let mut bytes = digest;
    bytes[6] = (bytes[6] & 0x0f) | 0x50; // version 5
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    uuid::Uuid::from_bytes(bytes)
}

pub(crate) fn event(
    actor: Actor,
    payload: EventPayload,
    subjects: Vec<SubjectRef>,
) -> VEvent {
    VEvent {
        event_id: Uuid::now_v7(),
        actor,
        timestamp: time::OffsetDateTime::now_utc(),
        subjects,
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        trace_id: None,
        payload,
    }
}
