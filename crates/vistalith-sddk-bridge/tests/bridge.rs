//! Governed promotion bridge invariants (SPK-012 / milestone M7): an intent
//! on an SDDK-owned subject goes through SDDK's own capability gateway. The
//! decision and the SDDK receipt are durable in Vistalith's log; SDDK's
//! ledger holds the receipt; Vistalith never writes SDDK state directly.

use std::path::PathBuf;

use vistalith_domain::{
    Actor, AuthorityClass, EventPayload, Namespace, SubjectDefined, SubjectKind, SubjectRef,
};
use vistalith_graph::GraphStore;
use vistalith_sddk_bridge::{ProposalDecision, SddkBridge};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn open_bridge(name: &str) -> SddkBridge {
    let ledger = std::env::temp_dir().join(format!(
        "vistalith-bridge-{}-{}.sqlite",
        name,
        uuid::Uuid::now_v7()
    ));
    SddkBridge::open(
        ledger,
        fixture_dir().join("workflow.json"),
        "TEST-PROJECT",
    )
    .expect("bridge opens")
}

fn actor() -> Actor {
    Actor::new("user:ruben").expect("static actor")
}

fn define_target(store: &mut GraphStore, id: &str) -> SubjectRef {
    let target =
        SubjectRef::new(Namespace::Sddk, SubjectKind::WorkItem, id.to_owned()).unwrap();
    store
        .append(vistalith_domain::VEvent {
            event_id: uuid::Uuid::now_v7(),
            actor: actor(),
            timestamp: time::OffsetDateTime::now_utc(),
            subjects: vec![target.clone()],
            correlation_id: uuid::Uuid::now_v7(),
            causation_id: None,
            trace_id: None,
            payload: EventPayload::SubjectDefined(SubjectDefined {
                subject: target.clone(),
                // SDDK truth is held as a derived observation in Vistalith.
                authority: AuthorityClass::Derived,
                provenance: vistalith_domain::Provenance::new("observation:sddk").unwrap(),
                properties: std::collections::BTreeMap::new(),
            }),
        })
        .unwrap();
    target
}

fn define_intent(store: &mut GraphStore, id: &str) -> SubjectRef {
    let intent =
        SubjectRef::new(Namespace::Visual, SubjectKind::VisualProposal, id.to_owned()).unwrap();
    store
        .append(vistalith_domain::VEvent {
            event_id: uuid::Uuid::now_v7(),
            actor: actor(),
            timestamp: time::OffsetDateTime::now_utc(),
            subjects: vec![intent.clone()],
            correlation_id: uuid::Uuid::now_v7(),
            causation_id: None,
            trace_id: None,
            payload: EventPayload::SubjectDefined(SubjectDefined {
                subject: intent.clone(),
                authority: AuthorityClass::Advisory,
                provenance: vistalith_domain::Provenance::new("user:ruben").unwrap(),
                properties: std::collections::BTreeMap::from([(
                    "change".to_owned(),
                    serde_json::json!({ "operations": [] }),
                )]),
            }),
        })
        .unwrap();
    intent
}

#[test]
fn allowed_proposal_executes_and_is_traced_end_to_end() {
    let bridge = open_bridge("allowed");
    let mut store = GraphStore::new();
    let target = define_target(&mut store, "TEST-MODEL-001");
    let intent = define_intent(&mut store, "intent-1");

    let proposal = bridge
        .submit_evidence_proposal(
            &mut store,
            &intent,
            &target,
            serde_json::json!({ "artifacts": [], "environment": {}, "execution": {} }),
            "payment service assessment from the visual workspace",
            &actor(),
            false,
        )
        .expect("allowed proposal");

    // Vistalith's log holds the derived proposal observation.
    let node = store.graph().subject(&proposal).unwrap();
    assert_eq!(node.subject.kind(), &SubjectKind::Proposal);
    assert_eq!(node.authority, AuthorityClass::Derived);
    assert_eq!(node.properties["decision"], serde_json::json!("allowed"));
    assert!(node.properties["receipt_id"].is_string());
    assert_eq!(
        node.properties["receipt"]["status"],
        serde_json::json!("succeeded"),
        "the SDDK ledger recorded a successful governed execution"
    );
    // The proposal is evidence for the SDDK work item it targets.
    assert!(store.graph().relations().any(|fact| {
        fact.relation.from == proposal
            && fact.relation.to == target
            && fact.relation.kind == vistalith_domain::RelationKind::ProvidesEvidenceFor
    }));

    // The receipt is durable in the SDDK ledger too (B1: direct core use).
    let receipts = bridge.receipts().expect("receipts read");
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0]["capability"], serde_json::json!("evidence.write"));

    // Replay determinism: the durable proposal events project identically.
    let stored = store.to_log_json();
    let replayed = GraphStore::from_stored_json(&stored).unwrap();
    assert_eq!(replayed.digest(), store.digest());
}

#[test]
fn high_risk_capability_requires_explicit_approval() {
    // A workflow declaring evidence.write as high risk: the gateway demands
    // approval before executing.
    let ledger = std::env::temp_dir().join(format!(
        "vistalith-bridge-approval-{}.sqlite",
        uuid::Uuid::now_v7()
    ));
    let workflow = r#"{
        "schema_version": 1,
        "workflow": { "id": "vistalith-bridge", "version": "0.1.0", "description": "high risk fixture" },
        "statuses": ["OPEN", "CLOSED"],
        "phases": ["explore"],
        "policies": {},
        "transitions": [{ "id": "open", "to": { "status": "OPEN" }, "requires": [] }],
        "forge": { "provider": "vistalith", "capabilities": {
            "evidence.write": { "risk": "high", "consequence": "creates" }
        } }
    }"#;
    let workflow_path = std::env::temp_dir().join(format!(
        "vistalith-workflow-high-risk-{}.json",
        uuid::Uuid::now_v7()
    ));
    std::fs::write(&workflow_path, workflow).unwrap();
    let bridge = SddkBridge::open(ledger, &workflow_path, "TEST-PROJECT").unwrap();

    let mut store = GraphStore::new();
    let target = define_target(&mut store, "TEST-9");
    let intent = define_intent(&mut store, "intent-2");

    // Without approval: the gateway demands it; nothing executes.
    let proposal = bridge
        .submit_evidence_proposal(
            &mut store,
            &intent,
            &target,
            serde_json::json!({}),
            "needs a human",
            &actor(),
            false,
        )
        .unwrap();
    let node = store.graph().subject(&proposal).unwrap();
    assert_eq!(node.properties["decision"], serde_json::json!("approval-required"));
    assert_eq!(node.properties["receipt_id"], serde_json::Value::Null);
    assert!(bridge.receipts().unwrap().is_empty());

    // With explicit approval: the gateway allows and executes.
    let proposal = bridge
        .submit_evidence_proposal(
            &mut store,
            &intent,
            &target,
            serde_json::json!({}),
            "human approved",
            &actor(),
            true,
        )
        .unwrap();
    let node = store.graph().subject(&proposal).unwrap();
    assert_eq!(node.properties["decision"], serde_json::json!("allowed"));
    assert!(node.properties["receipt_id"].is_string());
    assert_eq!(bridge.receipts().unwrap().len(), 1);
}

#[test]
fn undeclared_capabilities_are_denied_by_default() {
    // A workflow without evidence.write: default-deny applies.
    let ledger = std::env::temp_dir().join(format!(
        "vistalith-bridge-denied-{}.sqlite",
        uuid::Uuid::now_v7()
    ));
    let workflow = r#"{
        "schema_version": 1,
        "workflow": { "id": "vistalith-bridge-strict", "version": "0.1.0", "description": "strict fixture" },
        "statuses": ["OPEN", "CLOSED"],
        "phases": ["explore"],
        "policies": {},
        "transitions": [{ "id": "open", "to": { "status": "OPEN" }, "requires": [] }],
        "forge": { "provider": "vistalith", "capabilities": {
            "git.inspect": { "risk": "low", "consequence": "creates" }
        } }
    }"#;
    let workflow_path = std::env::temp_dir().join(format!(
        "vistalith-workflow-strict-{}.json",
        uuid::Uuid::now_v7()
    ));
    std::fs::write(&workflow_path, workflow).unwrap();
    let bridge = SddkBridge::open(ledger, &workflow_path, "TEST-PROJECT").unwrap();

    let mut store = GraphStore::new();
    let target = define_target(&mut store, "TEST-10");
    let intent = define_intent(&mut store, "intent-3");

    let proposal = bridge
        .submit_evidence_proposal(
            &mut store,
            &intent,
            &target,
            serde_json::json!({}),
            "should be denied",
            &actor(),
            false,
        )
        .unwrap();
    let node = store.graph().subject(&proposal).unwrap();
    assert_eq!(node.properties["decision"], serde_json::json!("denied"));
    assert_eq!(node.properties["receipt_id"], serde_json::Value::Null);
    assert!(
        matches!(
            node.properties["decision"],
            serde_json::Value::String(ref d) if d == ProposalDecision::Denied.wire()
        ),
        "the denial is durable and classified"
    );
    assert!(bridge.receipts().unwrap().is_empty());
}

// --- Workflow projection (slice 10, milestone M6) -----------------------------

use vistalith_sddk_bridge::SyncReport;

/// Inserts a cycle snapshot + causal event into the SDDK ledger, the way
/// the SDDK engine does (`insert_cycle_with_event`).
fn seed_cycle(
    ledger: &std::path::Path,
    cycle_id: &str,
    status: sddk_domain::CycleStatus,
    sequence: i64,
) {
    use sddk_domain::{CycleId, CycleManifest, LedgerEventInput, ProjectId};

    // Cycles FK on (project_id, workspace_id): register project + workspace
    // first, mirroring how the SDDK engine sets a project up.
    {
        let mut storage = sddk_storage::Storage::open(ledger).unwrap();
        let project = sddk_domain::ProjectRecord {
            project_id: "TEST-PROJECT".to_owned(),
            display_name: "smoke project".to_owned(),
            remote_url: None,
            scope: "vistalith-bridge".to_owned(),
            created_at: "2026-09-05T08:00:00Z".to_owned(),
        };
        let workspace = sddk_domain::WorkspaceRecord {
            workspace_id: "ws-1".to_owned(),
            project_id: "TEST-PROJECT".to_owned(),
            canonical_path: "/tmp/ws-1".to_owned(),
            created_at: "2026-09-05T08:00:00Z".to_owned(),
        };
        let _ = storage.register_project_workspace(&project, &workspace);
    }

    let manifest = CycleManifest::new(
        "TEST-PROJECT".to_owned(),
        "ws-1".to_owned(),
        CycleId::from_parts(&ProjectId::new("TEST-PROJECT").unwrap(), cycle_id).unwrap(),
        format!("cycle {cycle_id}"),
        format!("cycle/{cycle_id}"),
        "base-sha".to_owned(),
    );
    let manifest = CycleManifest {
        status,
        cycle_id: cycle_id.to_owned(),
        ..manifest
    };
    let now = "2026-09-05T08:00:00Z".to_owned();
    let record = sddk_domain::CycleRecord {
        manifest: manifest.clone(),
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let event = LedgerEventInput {
        event_id: uuid::Uuid::now_v7().to_string(),
        project_id: "TEST-PROJECT".to_owned(),
        cycle_id: Some(cycle_id.to_owned()),
        frame_id: format!("frame-{sequence}"),
        command_id: format!("cmd-{sequence}"),
        actor: "sddk-engine".to_owned(),
        event_type: "cycle.updated".to_owned(),
        occurred_at: now.clone(),
        state_before: None,
        state_after: Some(serde_json::to_value(&manifest).unwrap()),
        payload: serde_json::json!({}),
    };
    let mut storage = sddk_storage::Storage::open(ledger).unwrap();
    if storage.cycle_exists(cycle_id).unwrap() {
        storage
            .update_cycle_with_event(&manifest, &now, &event, false)
            .unwrap();
    } else {
        storage.insert_cycle_with_event(&record, &event).unwrap();
    }
}

#[test]
fn workflow_sync_projects_cycles_and_is_idempotent() {
    let ledger = std::env::temp_dir().join(format!(
        "vistalith-bridge-sync-{}.sqlite",
        uuid::Uuid::now_v7()
    ));
    let bridge = SddkBridge::open(
        &ledger,
        fixture_dir().join("workflow.json"),
        "TEST-PROJECT",
    )
    .expect("bridge opens");

    // Seed two cycles, then update one of them.
    seed_cycle(&ledger, "m1", sddk_domain::CycleStatus::Open, 1);
    seed_cycle(&ledger, "m1", sddk_domain::CycleStatus::ReleasePending, 2);
    seed_cycle(&ledger, "m2", sddk_domain::CycleStatus::Blocked, 3);

    let mut store = GraphStore::new();
    let report = bridge
        .sync_workflow(&mut store, &actor())
        .expect("first sync");

    // Two cycles projected + the project subject created.
    assert_eq!(report.subjects_created, 3, "report: {report:?}");
    let m1 = SubjectRef::new(Namespace::Sddk, SubjectKind::Workflow, "m1".to_owned()).unwrap();
    let node = store.graph().subject(&m1).unwrap();
    // The latest ledger event wins (VERIFYING over OPEN).
    assert_eq!(node.properties["status"], serde_json::json!("RELEASE_PENDING"));
    assert_eq!(node.properties["ledger_sequence"], serde_json::json!(2));
    let m2 = SubjectRef::new(Namespace::Sddk, SubjectKind::Workflow, "m2".to_owned()).unwrap();
    assert_eq!(
        store.graph().subject(&m2).unwrap().properties["status"],
        serde_json::json!("BLOCKED")
    );
    // The observed project exists and cycles derive from it.
    let project = SubjectRef::new(
        Namespace::Sddk,
        SubjectKind::Project,
        "TEST-PROJECT".to_owned(),
    )
    .unwrap();
    assert!(store.graph().subject(&project).is_some());
    assert!(store.graph().relations().any(|f| {
        f.relation.from == m1
            && f.relation.to == project
            && f.relation.kind == vistalith_domain::RelationKind::DerivesFrom
    }));

    // Idempotent: syncing again with no new ledger events appends nothing.
    let before = store.log().len();
    let again = bridge.sync_workflow(&mut store, &actor()).unwrap();
    let SyncReport {
        subjects_created,
        subjects_updated,
        events_skipped,
        ..
    } = again;
    assert_eq!(
        (subjects_created, subjects_updated, events_skipped),
        (0, 0, 2),
        "re-sync skips already-materialized state: {again:?}"
    );
    assert_eq!(store.log().len(), before);

    // A new ledger event updates the existing cycle subject in place.
    seed_cycle(&ledger, "m2", sddk_domain::CycleStatus::Open, 4);
    let report = bridge.sync_workflow(&mut store, &actor()).unwrap();
    assert_eq!(report.subjects_updated, 1);
    assert_eq!(
        store.graph().subject(&m2).unwrap().properties["status"],
        serde_json::json!("OPEN")
    );

    // Replay determinism over the synced log.
    let stored = store.to_log_json();
    let replayed = GraphStore::from_stored_json(&stored).unwrap();
    assert_eq!(replayed.digest(), store.digest());
}
