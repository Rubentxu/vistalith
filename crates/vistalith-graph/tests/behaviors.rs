//! Reactive behavior invariants (SPEC-003, milestone M4): a code-change
//! observation triggers a deterministic relation behavior that raises an
//! advisory impact proposal, traceable via `causation_id` in the log itself.
//! Replay never re-runs behaviors — their outputs are durable events — so
//! replay stays byte-deterministic.

use std::collections::BTreeMap;

use vistalith_domain::{
    Actor, AuthorityClass, EventPayload, Namespace, Provenance, RelationDeclared, RelationFact,
    RelationKind, RelationRef, SubjectDefined, SubjectDeprecated, SubjectKind, SubjectRef,
    SubjectUpdated, VEvent,
};
use vistalith_graph::{Behavior, BehaviorContext, GraphStore, builtin_behaviors, append_and_react};

fn actor() -> Actor {
    Actor::new("observation:ci").expect("static actor")
}

fn container(id: &str) -> SubjectRef {
    SubjectRef::new(Namespace::Arch, SubjectKind::Container, id).unwrap()
}

fn event(payload: EventPayload) -> VEvent {
    VEvent {
        event_id: uuid::Uuid::now_v7(),
        actor: actor(),
        timestamp: time::OffsetDateTime::now_utc(),
        subjects: Vec::new(),
        correlation_id: uuid::Uuid::now_v7(),
        causation_id: None,
        trace_id: None,
        payload,
    }
}

fn define(graph: &mut GraphStore, subject: SubjectRef) {
    graph
        .append(event(EventPayload::SubjectDefined(SubjectDefined {
            subject,
            authority: AuthorityClass::Authoritative,
            provenance: Provenance::new("observation:ci").unwrap(),
            properties: BTreeMap::new(),
        })))
        .unwrap();
}

fn depends_on(from: &SubjectRef, to: &SubjectRef) -> VEvent {
    event(EventPayload::RelationDeclared(RelationDeclared {
        fact: RelationFact {
            relation: RelationRef::new(from.clone(), RelationKind::DependsOn, to.clone()).unwrap(),
            authority: AuthorityClass::Authoritative,
            provenance: Provenance::new("observation:ci").unwrap(),
        },
    }))
}

#[test]
fn dependency_change_raises_traced_impact_advisories() {
    let mut store = GraphStore::new();
    define(&mut store, container("payment-service"));
    define(&mut store, container("ledger"));
    define(&mut store, container("gateway"));
    // payment-service depends_on ledger; gateway depends_on payment-service.
    store.append(depends_on(&container("payment-service"), &container("ledger"))).unwrap();
    store.append(depends_on(&container("gateway"), &container("payment-service"))).unwrap();

    // The dependency (ledger) changes: the direct dependent gets an advisory.
    let trigger = event(EventPayload::SubjectUpdated(SubjectUpdated {
        subject: container("ledger"),
        properties: BTreeMap::new(),
    }));
    let outcome = append_and_react(&mut store, trigger, &builtin_behaviors()).unwrap();
    assert_eq!(outcome.advisories, 1, "one direct dependent: payment-service");

    // The advisory is a durable, advisory-class subject linked to the
    // dependent, with trace back to the trigger.
    let trigger_event = &store.log()[store.log().len() - 2];
    let advisory_event = store.log().last().unwrap();
    assert_eq!(advisory_event.event.kind(), "advisory-raised");
    assert_eq!(
        advisory_event.event.causation_id,
        Some(trigger_event.event.event_id),
        "SPEC-003: the advisory traces to its trigger"
    );
    assert_eq!(advisory_event.event.correlation_id, trigger_event.event.correlation_id);

    let EventPayload::AdvisoryRaised(raised) = &advisory_event.event.payload else {
        panic!("expected advisory payload");
    };
    assert_eq!(raised.about, container("payment-service"));
    assert!(raised.note.contains("depends_on"));
    let node = store.graph().subject(&raised.advisory).unwrap();
    assert_eq!(node.authority, AuthorityClass::Advisory);

    // RelationBehavior contract: relation-kind attachment is explicit in the
    // spec; a non-dependency change raises nothing.
    let outcome = append_and_react(
        &mut store,
        event(EventPayload::SubjectUpdated(SubjectUpdated {
            subject: container("gateway"),
            properties: BTreeMap::new(),
        })),
        &builtin_behaviors(),
    )
    .unwrap();
    assert_eq!(outcome.advisories, 0, "nothing depends on gateway");
}

#[test]
fn deprecated_evidence_and_contradictions_surface_advisories() {
    let mut store = GraphStore::new();
    let evidence = SubjectRef::new(Namespace::Verification, SubjectKind::Evidence, "bench-1")
        .unwrap();
    let decision = SubjectRef::new(Namespace::Sddk, SubjectKind::Decision, "D-1").unwrap();
    define(&mut store, evidence.clone());
    define(&mut store, decision.clone());
    store
        .append(event(EventPayload::RelationDeclared(RelationDeclared {
            fact: RelationFact {
                relation: RelationRef::new(
                    evidence.clone(),
                    RelationKind::ProvidesEvidenceFor,
                    decision.clone(),
                )
                .unwrap(),
                authority: AuthorityClass::Authoritative,
                provenance: Provenance::new("observation:ci").unwrap(),
            },
        })))
        .unwrap();

    // Deprecating the evidence raises a stale-evidence advisory on the
    // supported decision.
    let outcome = append_and_react(
        &mut store,
        event(EventPayload::SubjectDeprecated(SubjectDeprecated {
            subject: evidence,
            reason: Some("benchmark invalidated".to_owned()),
        })),
        &builtin_behaviors(),
    )
    .unwrap();
    assert_eq!(outcome.advisories, 1);
    let last = store.log().last().unwrap();
    let EventPayload::AdvisoryRaised(advisory) = &last.event.payload else {
        panic!("expected advisory");
    };
    assert_eq!(advisory.about, decision);
    assert!(advisory.note.contains("stale evidence"));

    // A contradicts edge surfaces a conflict advisory.
    let a = container("option-a");
    let b = container("option-b");
    define(&mut store, a.clone());
    define(&mut store, b.clone());
    let outcome = append_and_react(
        &mut store,
        event(EventPayload::RelationDeclared(RelationDeclared {
            fact: RelationFact {
                relation: RelationRef::new(a, RelationKind::Contradicts, b).unwrap(),
                authority: AuthorityClass::Advisory,
                provenance: Provenance::new("observation:ci").unwrap(),
            },
        })),
        &builtin_behaviors(),
    )
    .unwrap();
    assert_eq!(outcome.advisories, 1);
}

#[test]
fn observed_work_item_without_evidence_gets_a_one_time_advisory() {
    let mut store = GraphStore::new();
    let item = SubjectRef::new(Namespace::Sddk, SubjectKind::WorkItem, "TEST-77").unwrap();
    let outcome = append_and_react(
        &mut store,
        event(EventPayload::SubjectDefined(SubjectDefined {
            subject: item.clone(),
            authority: AuthorityClass::Derived,
            provenance: Provenance::new("observation:sddk").unwrap(),
            properties: BTreeMap::new(),
        })),
        &builtin_behaviors(),
    )
    .unwrap();
    assert_eq!(outcome.advisories, 1, "work item lacks evidence");
    let EventPayload::AdvisoryRaised(advisory) = &store.log().last().unwrap().event.payload
    else {
        panic!("expected advisory");
    };
    assert_eq!(advisory.about, item);
}

#[test]
fn behaviors_may_not_emit_non_advisory_payloads() {
    let mut store = GraphStore::new();
    define(&mut store, container("svc"));
    let behaviors: Vec<Box<dyn Behavior>> = vec![Box::new(RogueBehavior)];
    let err = append_and_react(
        &mut store,
        event(EventPayload::SubjectUpdated(SubjectUpdated {
            subject: container("svc"),
            properties: BTreeMap::new(),
        })),
        &behaviors,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        vistalith_graph::StoreError::BehaviorOutputRejected(_)
    ));
    // The trigger itself is still durable; the rogue output is not in the log.
    assert_eq!(store.log().last().unwrap().event.kind(), "subject-updated");
}

struct RogueBehavior;

impl Behavior for RogueBehavior {
    fn spec(&self) -> vistalith_graph::BehaviorSpec {
        vistalith_graph::BehaviorSpec {
            name: "rogue".to_owned(),
            version: 1,
            subscribes: &["subject-updated"],
            relation_kinds: &[],
            determinism: vistalith_domain::DeterminismClass::DeterministicRule,
        }
    }
    fn react(&self, _ctx: &BehaviorContext) -> Vec<EventPayload> {
        // Tries to define an authoritative SDDK subject — the guardrail must
        // reject this (SPEC-003: behaviors never mutate SDDK truth).
        vec![EventPayload::SubjectDefined(SubjectDefined {
            subject: SubjectRef::new(Namespace::Sddk, SubjectKind::WorkItem, "ROGUE").unwrap(),
            authority: AuthorityClass::Authoritative,
            provenance: Provenance::new("behavior:rogue@1").unwrap(),
            properties: BTreeMap::new(),
        })]
    }
}

#[test]
fn replay_does_not_rerun_behaviors_and_stays_deterministic() {
    // Build a store with behaviors live...
    let mut live = GraphStore::new();
    define(&mut live, container("a"));
    define(&mut live, container("b"));
    live.append(depends_on(&container("a"), &container("b"))).unwrap();
    append_and_react(
        &mut live,
        event(EventPayload::SubjectUpdated(SubjectUpdated {
            subject: container("b"),
            properties: BTreeMap::new(),
        })),
        &builtin_behaviors(),
    )
    .unwrap();

    // ...then replay its log with an EMPTY behavior set: the durable advisory
    // events project like any other event, deterministically.
    let stored = live.to_log_json();
    let replayed = vistalith_graph::GraphStore::from_stored_json(&stored).unwrap();
    assert_eq!(replayed.digest(), live.digest());
    let advisories = replayed
        .graph()
        .subjects_of_kind(&SubjectKind::Advisory)
        .count();
    assert_eq!(advisories, 1, "the durable advisory survives replay");
}
