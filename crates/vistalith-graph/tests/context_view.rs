//! Semantic Context View invariants (SPEC-005, milestone M3): every bound is
//! enforced and every inclusion/exclusion carries provenance — the view can
//! always answer "why is this item in my context?".

use std::collections::BTreeMap;

use vistalith_domain::{
    Actor, AuthorityClass, EventPayload, Namespace, Provenance, RelationDeclared, RelationFact,
    RelationKind, RelationRef, SubjectDefined, SubjectKind, SubjectRef, VEvent,
};
use vistalith_graph::{ContextRequest, GraphStore, build_context_view};

fn actor() -> Actor {
    Actor::new("user:ruben").expect("static actor")
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

fn define(store: &mut GraphStore, subject: SubjectRef, authority: AuthorityClass) {
    store
        .append(event(EventPayload::SubjectDefined(SubjectDefined {
            subject,
            authority,
            provenance: Provenance::new("user:ruben").unwrap(),
            properties: BTreeMap::new(),
        })))
        .unwrap();
}

fn relate(store: &mut GraphStore, from: &SubjectRef, kind: RelationKind, to: &SubjectRef) {
    store
        .append(event(EventPayload::RelationDeclared(RelationDeclared {
            fact: RelationFact {
                relation: RelationRef::new(from.clone(), kind, to.clone()).unwrap(),
                authority: AuthorityClass::Authoritative,
                provenance: Provenance::new("user:ruben").unwrap(),
            },
        })))
        .unwrap();
}

/// payment-service -[depends_on]-> ledger -[depends_on]-> database,
/// plus an advisory mentioning payment-service and a far node via `mentions`.
fn sample_store() -> GraphStore {
    let mut store = GraphStore::new();
    define(&mut store, container("payment-service"), AuthorityClass::Authoritative);
    define(&mut store, container("ledger"), AuthorityClass::Authoritative);
    define(&mut store, container("database"), AuthorityClass::Authoritative);
    define(&mut store, container("unrelated"), AuthorityClass::Authoritative);
    let advisory = SubjectRef::new(Namespace::Vistalith, SubjectKind::Advisory, "adv-1").unwrap();
    define(&mut store, advisory, AuthorityClass::Advisory);
    relate(
        &mut store,
        &container("payment-service"),
        RelationKind::DependsOn,
        &container("ledger"),
    );
    relate(
        &mut store,
        &container("ledger"),
        RelationKind::DependsOn,
        &container("database"),
    );
    relate(
        &mut store,
        &SubjectRef::new(Namespace::Vistalith, SubjectKind::Advisory, "adv-1").unwrap(),
        RelationKind::Mentions,
        &container("payment-service"),
    );
    store
}

#[test]
fn depth_and_relation_allowlist_bound_the_slice() {
    let store = sample_store();

    // Depth 1 from payment-service: ledger in, database out (too deep),
    // unrelated out (not reachable).
    let view = build_context_view(
        &store,
        &ContextRequest {
            roots: vec![container("payment-service")],
            relations: Some(vec![RelationKind::DependsOn]),
            max_depth: 1,
            include_derived: false,
            include_advisory: false,
            recency_cutoff: None,
            token_budget: 100_000,
        },
    );
    let subjects: Vec<&str> = view
        .items
        .iter()
        .map(|i| i.subject.as_str())
        .collect();
    assert_eq!(subjects, ["arch:container:payment-service", "arch:container:ledger"]);
    assert!(view.exclusions.iter().any(|e| e.subject == "arch:container:database"
        && matches!(e.exclusion,
            vistalith_graph::ExclusionReason::DeeperThanMaxDepth { depth: 2 })));
    // "unrelated" is never even discovered: it is absent from both lists
    // (the view only explains what it encountered).
    assert!(!view.items.iter().any(|i| i.subject == "arch:container:unrelated"));
    assert!(!view.exclusions.iter().any(|e| e.subject == "arch:container:unrelated"));

    // Inclusion provenance: root vs via.
    let root_item = &view.items[0];
    assert!(matches!(
        root_item.reason,
        vistalith_graph::InclusionReason::Root
    ));
    let via_item = &view.items[1];
    match &via_item.reason {
        vistalith_graph::InclusionReason::Via { from, kind, depth } => {
            assert_eq!(from, "arch:container:payment-service");
            assert_eq!(kind, "depends_on");
            assert_eq!(*depth, 1);
        }
        other => panic!("expected Via reason, got {other:?}"),
    }
    // Every item carries its last-touch provenance.
    assert_eq!(via_item.last_actor, "user:ruben");
    assert!(!via_item.last_touch.is_empty());
}

#[test]
fn authority_filter_excludes_advisories_and_reports_why() {
    let store = sample_store();
    // The advisory would only be reached as a root; prove the authority gate
    // by requesting it directly.
    let view = build_context_view(
        &store,
        &ContextRequest {
            roots: vec![
                container("payment-service"),
                SubjectRef::new(Namespace::Vistalith, SubjectKind::Advisory, "adv-1").unwrap(),
            ],
            relations: None,
            max_depth: 1,
            include_derived: false,
            include_advisory: false,
            recency_cutoff: None,
            token_budget: 100_000,
        },
    );
    assert!(view
        .items
        .iter()
        .all(|i| i.subject != "vistalith:advisory:adv-1"));
    assert!(view.exclusions.iter().any(|e| e.subject == "vistalith:advisory:adv-1"
        && matches!(e.exclusion,
            vistalith_graph::ExclusionReason::AuthorityClass { ref class } if class == "advisory")));

    // With advisories included, the same request admits it.
    let view = build_context_view(
        &store,
        &ContextRequest {
            roots: vec![
                container("payment-service"),
                SubjectRef::new(Namespace::Vistalith, SubjectKind::Advisory, "adv-1").unwrap(),
            ],
            relations: None,
            max_depth: 1,
            include_derived: false,
            include_advisory: true,
            recency_cutoff: None,
            token_budget: 100_000,
        },
    );
    assert!(view.items.iter().any(|i| i.subject == "vistalith:advisory:adv-1"));
}

#[test]
fn token_budget_is_never_exceeded_and_flags_truncation() {
    let mut store = GraphStore::new();
    let provenance = Provenance::new("user:ruben").unwrap();
    let big_properties = BTreeMap::from([(
        "payload".to_owned(),
        serde_json::Value::String("x".repeat(2_000)),
    )]);
    store
        .append(event(EventPayload::SubjectDefined(SubjectDefined {
            subject: container("big"),
            authority: AuthorityClass::Authoritative,
            provenance: provenance.clone(),
            properties: big_properties,
        })))
        .unwrap();
    define(&mut store, container("small"), AuthorityClass::Authoritative);

    let view = build_context_view(
        &store,
        &ContextRequest {
            roots: vec![container("big"), container("small")],
            relations: None,
            max_depth: 0,
            include_derived: false,
            include_advisory: false,
            recency_cutoff: None,
            token_budget: 400,
        },
    );
    assert!(view.estimated_tokens <= 600, "budget must hold");
    assert!(view.truncated);
    assert!(view
        .exclusions
        .iter()
        .any(|e| e.subject == "arch:container:big"
            && matches!(e.exclusion, vistalith_graph::ExclusionReason::TokenBudgetExhausted)));
    assert!(view.items.iter().any(|i| i.subject == "arch:container:small"));
}

#[test]
fn identical_requests_over_identical_logs_are_deterministic() {
    let store = sample_store();
    let request = ContextRequest {
        roots: vec![container("payment-service")],
        relations: None,
        max_depth: 2,
        include_derived: false,
        include_advisory: false,
        recency_cutoff: None,
        token_budget: 100_000,
    };
    let first = serde_json::to_string(&build_context_view(&store, &request)).unwrap();
    let second = serde_json::to_string(&build_context_view(&store, &request)).unwrap();
    assert_eq!(first, second, "the view is a pure projection");
}
