//! Full impact analysis invariants (visual/IMPACT.md, slice 16): direct and
//! transitive dependents, affected tests/evidence/decisions, and explicit
//! unknown-impact representation. Advisory output — never a mutation.

use vistalith_domain::{Namespace, SubjectKind, SubjectRef};
use vistalith_graph::{AlgorithmGraph, GraphStore};

fn arch(id: &str) -> SubjectRef {
    SubjectRef::new(Namespace::Arch, SubjectKind::Container, id.to_owned()).unwrap()
}

fn code(id: &str) -> SubjectRef {
    SubjectRef::new(Namespace::Code, SubjectKind::Symbol, id.to_owned()).unwrap()
}

fn verification(kind: SubjectKind, id: &str) -> SubjectRef {
    SubjectRef::new(Namespace::Verification, kind, id.to_owned()).unwrap()
}

/// payment-service <-[implements]- pay-code <-[depends_on]- client,
/// pay-code -[tested_by... via depends_on]- pay-tests, plus evidence for
/// the decision and an unknown-kind supporter.
fn fixture() -> GraphStore {
    GraphStore::new()
}

#[test]
fn impact_analysis_sections_are_explicit() {
    // Build a chain with mixed kinds through the projection.
    let mut store = fixture();
    let mut append = |payload: vistalith_domain::EventPayload| {
        store
            .append(vistalith_domain::VEvent {
                event_id: uuid::Uuid::now_v7(),
                actor: vistalith_domain::Actor::new("user:ruben").unwrap(),
                timestamp: time::OffsetDateTime::now_utc(),
                subjects: Vec::new(),
                correlation_id: uuid::Uuid::now_v7(),
                causation_id: None,
                trace_id: None,
                payload,
            })
            .unwrap();
    };
    use vistalith_domain::{EventPayload, RelationKind, SubjectDefined, SubjectRef as SR};
    let mut define = |subject: SR| {
        append(EventPayload::SubjectDefined(SubjectDefined {
            subject,
            authority: vistalith_domain::AuthorityClass::Authoritative,
            provenance: vistalith_domain::Provenance::new("user:ruben").unwrap(),
            properties: Default::default(),
        }));
    };
    define(arch("payment-service"));
    define(code("pay-code"));
    define(code("client"));
    define(verification(SubjectKind::Test, "pay-tests"));
    define(verification(SubjectKind::Evidence, "bench-pay"));
    define(verification(SubjectKind::Decision, "d-pay"));
    define(verification(SubjectKind::Hypothesis, "h-unknown"));
    let mut rel = |from: SR, kind: vistalith_domain::RelationKind, to: SR| {
        append(EventPayload::RelationDeclared(
            vistalith_domain::RelationDeclared {
                fact: vistalith_domain::RelationFact {
                    relation: vistalith_domain::RelationRef::new(from, kind, to).unwrap(),
                    authority: vistalith_domain::AuthorityClass::Authoritative,
                    provenance: vistalith_domain::Provenance::new("user:ruben").unwrap(),
                },
            },
        ));
    };
    rel(code("pay-code"), RelationKind::Implements, arch("payment-service"));
    rel(code("pay-code"), RelationKind::DependsOn, code("client"));
    rel(
        verification(SubjectKind::Test, "pay-tests"),
        RelationKind::DependsOn,
        code("pay-code"),
    );
    rel(
        verification(SubjectKind::Evidence, "bench-pay"),
        RelationKind::ProvidesEvidenceFor,
        code("pay-code"),
    );
    rel(
        verification(SubjectKind::Decision, "d-pay"),
        RelationKind::DependsOn,
        code("pay-code"),
    );
    rel(
        verification(SubjectKind::Hypothesis, "h-unknown"),
        RelationKind::DependsOn,
        code("pay-code"),
    );

    let snapshot = AlgorithmGraph::extract(store.graph(), None);
    let analysis = snapshot
        .impact_analysis(&code("pay-code"), true)
        .expect("root exists");

    // Direct: everything depending one hop back, sorted.
    assert_eq!(
        analysis.direct_dependents,
        vec![
            "verification:decision:d-pay",
            "verification:evidence:bench-pay",
            "verification:hypothesis:h-unknown",
            "verification:test:pay-tests",
        ]
    );
    // Tests surface in their own section.
    assert!(analysis.affected_tests.contains(&"verification:test:pay-tests".to_owned()));
    // Evidence surfaces as stale-evidence.
    assert!(
        analysis
            .stale_evidence
            .contains(&"verification:evidence:bench-pay".to_owned())
    );
    // Decisions whose basis is affected.
    assert!(
        analysis
            .decisions_potentially_invalidated
            .contains(&"verification:decision:d-pay".to_owned())
    );
    // Unknown impact is explicit, never hidden.
    assert!(
        analysis
            .unknown_impact
            .contains(&"verification:hypothesis:h-unknown".to_owned())
    );
}
