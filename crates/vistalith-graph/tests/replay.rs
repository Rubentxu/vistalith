use std::path::PathBuf;

use vistalith_domain::{
    Actor, AuthorityClass, Namespace, PatchId, PatchOperation, Provenance, RelationFact,
    RelationKind, SubjectKind, SubjectRef,
};
use vistalith_graph::{GraphPatch, GraphStore, PatchOutcome, StoreError};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-world.json")
}

fn loaded() -> GraphStore {
    GraphStore::from_fixture_path(fixture_path()).expect("fixture replays cleanly")
}

fn container(id: &str) -> SubjectRef {
    SubjectRef::new(Namespace::Arch, SubjectKind::Container, id).unwrap()
}

#[test]
fn fixture_replays_deterministically() {
    let a = loaded();
    let b = loaded();

    assert_eq!(a.graph().subject_count(), 3);
    assert_eq!(a.graph().relation_count(), 2);
    assert_eq!(a.graph().revision(), 5, "five state-changing events");

    // Determinism: two replays of the same log produce identical state.
    assert_eq!(a.digest(), b.digest());
}

#[test]
fn graph_is_reconstructible_from_durable_log() {
    let store = loaded();
    let stored_json = store.to_log_json();

    let rebuilt = GraphStore::from_stored_json(&stored_json).expect("stored log rebuilds");
    assert_eq!(rebuilt.digest(), store.digest());

    // The raw fixture alone reconstructs the same graph: durable sources are
    // the events, not the materialized view.
    let raw = std::fs::read_to_string(fixture_path()).unwrap();
    assert_eq!(
        GraphStore::from_raw_json(&raw).unwrap().digest(),
        store.digest()
    );
}

#[test]
fn cross_lens_selection_uses_subject_refs_not_renderer_ids() {
    let store = loaded();

    let container = store
        .graph()
        .subject(&container("payment-service"))
        .expect("container is in the graph");
    assert_eq!(container.authority, AuthorityClass::Authoritative);

    let work_item = SubjectRef::parse("sddk:work-item:TEST-MODEL-001").unwrap();
    let node = store.graph().subject(&work_item).unwrap();
    assert_eq!(node.authority, AuthorityClass::Derived);
    assert!(
        !node.is_sddk_owned(),
        "observations are derived, not authoritative"
    );

    // The same SubjectRef is reachable through relation endpoints.
    let affects = RelationKind::parse("affects").unwrap();
    let fact = store
        .graph()
        .relations_of_kind(&affects)
        .next()
        .expect("advisory affects edge exists");
    assert_eq!(fact.relation.from, work_item);
    assert_eq!(fact.authority, AuthorityClass::Advisory);
    assert_eq!(fact.provenance.confidence, Some(0.6));
}

#[test]
fn patch_lifecycle_proposed_applied_then_stale_rejected() {
    let mut store = loaded();
    let base = store.graph().revision();

    let hypothesis = SubjectRef::new(Namespace::Visual, SubjectKind::Hypothesis, "h-001").unwrap();
    let patch = GraphPatch {
        patch_id: PatchId::new("patch-apply-me").unwrap(),
        base_revision: base,
        proposed_by: Actor::new("agent:planner").unwrap(),
        operations: vec![PatchOperation::UpsertSubject {
            subject: hypothesis.clone(),
            authority: AuthorityClass::Advisory,
            provenance: Provenance::new("agent:planner").unwrap(),
            properties: Default::default(),
        }],
    };

    match store.propose_patch(patch).unwrap() {
        PatchOutcome::Applied { revision, .. } => {
            assert_eq!(revision, base + 1);
        }
        PatchOutcome::Rejected { reason, .. } => panic!("expected applied, got: {reason}"),
    }
    assert!(store.graph().subject(&hypothesis).is_some());
    assert_eq!(store.log().last().unwrap().event.kind(), "patch-applied");

    // Same base again -> stale -> rejected, and the rejection is durable.
    let stale = GraphPatch {
        patch_id: PatchId::new("patch-stale").unwrap(),
        base_revision: base,
        proposed_by: Actor::new("agent:planner").unwrap(),
        operations: vec![PatchOperation::DeprecateSubject {
            subject: hypothesis,
            reason: None,
        }],
    };
    match store.propose_patch(stale).unwrap() {
        PatchOutcome::Rejected { reason, .. } => {
            assert!(reason.contains("stale base revision"), "{reason}");
        }
        PatchOutcome::Applied { .. } => panic!("stale patch must not apply"),
    }
    assert_eq!(
        store.graph().revision(),
        base + 1,
        "rejections do not bump the graph"
    );
    assert_eq!(store.log().last().unwrap().event.kind(), "patch-rejected");
}

#[test]
fn authoritative_sddk_mutations_are_converted_not_applied() {
    let mut store = loaded();
    let base = store.graph().revision();

    // Try to authoritatively redefine SDDK-owned truth from Vistalith.
    let patch = GraphPatch {
        patch_id: PatchId::new("patch-sddk-takeover").unwrap(),
        base_revision: base,
        proposed_by: Actor::new("agent:planner").unwrap(),
        operations: vec![PatchOperation::UpsertSubject {
            subject: SubjectRef::new(Namespace::Sddk, SubjectKind::WorkItem, "TEST-MODEL-002")
                .unwrap(),
            authority: AuthorityClass::Authoritative,
            provenance: Provenance::new("agent:planner").unwrap(),
            properties: Default::default(),
        }],
    };

    match store.propose_patch(patch).unwrap() {
        PatchOutcome::Rejected { reason, .. } => {
            assert!(
                reason.contains("governed SDDK semantic proposal"),
                "{reason}"
            );
        }
        PatchOutcome::Applied { .. } => panic!("SDDK takeover must not apply"),
    }
    assert_eq!(store.graph().revision(), base);
    assert_eq!(
        store.graph().subject_count(),
        3,
        "rejected ops leave no residue"
    );
}

#[test]
fn derived_relation_touching_sddk_owned_subject_is_allowed_but_authoritative_is_not() {
    let mut store = loaded();
    let base = store.graph().revision();
    let work_item = SubjectRef::parse("sddk:work-item:TEST-MODEL-001").unwrap();
    let repo = SubjectRef::new(Namespace::Code, SubjectKind::Repository, "payments-api").unwrap();

    // Advisory/derived edges are observations: allowed.
    let observation = GraphPatch {
        patch_id: PatchId::new("patch-observe").unwrap(),
        base_revision: base,
        proposed_by: Actor::new("agent:analyzer").unwrap(),
        operations: vec![PatchOperation::DeclareRelation {
            fact: RelationFact {
                relation: vistalith_domain::RelationRef::new(
                    repo.clone(),
                    RelationKind::TestedBy,
                    work_item.clone(),
                )
                .unwrap(),
                authority: AuthorityClass::Derived,
                provenance: Provenance::new("agent:analyzer").unwrap(),
            },
        }],
    };
    assert!(matches!(
        store.propose_patch(observation).unwrap(),
        PatchOutcome::Applied { .. }
    ));

    // Now an authoritative edge into the SDDK-owned subject: rejected.
    let takeover = GraphPatch {
        patch_id: PatchId::new("patch-authoritative-edge").unwrap(),
        base_revision: store.graph().revision(),
        proposed_by: Actor::new("agent:analyzer").unwrap(),
        operations: vec![PatchOperation::DeclareRelation {
            fact: RelationFact {
                relation: vistalith_domain::RelationRef::new(
                    work_item.clone(),
                    RelationKind::Blocks,
                    container("payment-service"),
                )
                .unwrap(),
                authority: AuthorityClass::Authoritative,
                provenance: Provenance::new("agent:analyzer").unwrap(),
            },
        }],
    };
    match store.propose_patch(takeover).unwrap() {
        PatchOutcome::Rejected { reason, .. } => {
            assert!(reason.contains("SDDK-owned"), "{reason}");
        }
        PatchOutcome::Applied { .. } => {
            panic!("authoritative edge into SDDK-owned subject must not apply")
        }
    }
}

#[test]
fn duplicate_event_ids_are_rejected_idempotently() {
    let mut store = loaded();
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture_path()).unwrap()).unwrap();
    let duplicate: vistalith_domain::VEvent =
        serde_json::from_value(raw["events"][0].clone()).expect("fixture event deserializes");

    match store.append(duplicate) {
        Err(StoreError::DuplicateEventId(id)) => {
            assert_eq!(store.log().len(), 5);
            assert_eq!(id.to_string(), "0198f6c0-0000-7000-8000-000000000001");
        }
        other => panic!("expected duplicate id rejection, got {other:?}"),
    }
}
