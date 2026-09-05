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
