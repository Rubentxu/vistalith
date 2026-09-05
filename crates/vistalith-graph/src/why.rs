//! The "why" path (milestone M9, `visual/TRACEABILITY-WHY.md`): walk
//! backwards from a subject through its supporting relations and answer
//! "what is this based on?" — decisions, requirements, evidence, provenance.
//!
//! The walk follows *incoming* edges only: support flows towards the
//! subject (evidence provides_evidence_for a decision; code implements an
//! architecture subject; an advisory mentions it). Deterministic: ties are
//! broken by (edge kind, source identity).

use serde::Serialize;
use std::collections::{BTreeSet, HashMap, VecDeque};
use vistalith_domain::SubjectRef;

use crate::graph::SemanticWorldGraph;

#[derive(Debug, Clone, Serialize)]
pub struct WhyLink {
    pub depth: u8,
    /// Edge kind that justifies the connection, e.g. `provides_evidence_for`.
    pub kind: String,
    /// The supporting subject.
    pub from: String,
    /// The supported subject (closer to the asked-about subject).
    pub to: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhyPath {
    pub subject: String,
    /// Supporting links, breadth-first from the subject, ordered by
    /// (depth, kind, from).
    pub links: Vec<WhyLink>,
    /// Evidence-class links (provides_evidence_for, verifies): the
    /// hard-support backbone.
    pub evidence: Vec<WhyLink>,
    pub max_depth_reached: u8,
}

/// Relation kinds that count as hard evidence.
const EVIDENCE_KINDS: [&str; 2] = ["provides_evidence_for", "verifies"];

pub fn why_path(
    graph: &SemanticWorldGraph,
    subject: &SubjectRef,
    max_depth: u8,
) -> Option<WhyPath> {
    graph.subject(subject)?;

    // Index incoming edges by target for the walk.
    let mut incoming: HashMap<String, Vec<(&str, &SubjectRef)>> = HashMap::new();
    for fact in graph.relations() {
        incoming
            .entry(fact.relation.to.to_string())
            .or_default()
            .push((fact.relation.kind.as_str(), &fact.relation.from));
    }

    let mut links: Vec<WhyLink> = Vec::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    visited.insert(subject.to_string());
    let mut queue: VecDeque<(String, u8)> = VecDeque::new();
    queue.push_back((subject.to_string(), 0));
    let mut max_depth_reached = 0u8;

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        let mut supporters = incoming.remove(&current).unwrap_or_default();
        // Deterministic order: (kind, from).
        supporters.sort_by(|(kind_a, from_a), (kind_b, from_b)| {
            kind_a.cmp(kind_b).then(from_a.cmp(from_b))
        });
        for (kind, from) in supporters {
            let from_string = from.to_string();
            if visited.contains(&from_string) {
                continue;
            }
            visited.insert(from_string.clone());
            max_depth_reached = max_depth_reached.max(depth + 1);
            links.push(WhyLink {
                depth: depth + 1,
                kind: kind.to_owned(),
                from: from_string.clone(),
                to: current.clone(),
            });
            queue.push_back((from_string, depth + 1));
        }
    }

    links.sort_by(|a, b| {
        a.depth
            .cmp(&b.depth)
            .then(a.kind.cmp(&b.kind))
            .then(a.from.cmp(&b.from))
    });
    let evidence: Vec<WhyLink> = links
        .iter()
        .filter(|link| EVIDENCE_KINDS.contains(&link.kind.as_str()))
        .cloned()
        .collect();

    Some(WhyPath {
        subject: subject.to_string(),
        links,
        evidence,
        max_depth_reached,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use vistalith_domain::{
        AuthorityClass, Namespace, Provenance, RelationDeclared, RelationFact, RelationKind,
        RelationRef, SubjectDefined, SubjectKind,
    };
    use crate::projection::apply_event;
    use vistalith_domain::VEvent;

    fn subject(kind: SubjectKind, id: &str) -> SubjectRef {
        SubjectRef::new(Namespace::Arch, kind, id.to_owned()).unwrap()
    }

    fn graph_with_chain() -> SemanticWorldGraph {
        // decision <-[provides_evidence_for]- evidence,
        // decision <-[implements]- code,
        // code <-[depends_on]- client
        let mut graph = SemanticWorldGraph::new();
        let provenance = Provenance::new("test:why").unwrap();
        let decision = subject(SubjectKind::Container, "decision-svc");
        let evidence = SubjectRef::new(
            Namespace::Verification,
            SubjectKind::Evidence,
            "bench-1".to_owned(),
        )
        .unwrap();
        let code = subject(SubjectKind::Symbol, "decision-code");
        let client = subject(SubjectKind::Container, "client");
        for subject in [&decision, &code, &client] {
            apply_event(
                &mut graph,
                &VEvent {
                    event_id: uuid::Uuid::now_v7(),
                    actor: actor("test:why"),
                    timestamp: time::OffsetDateTime::now_utc(),
                    subjects: vec![subject.clone()],
                    correlation_id: uuid::Uuid::now_v7(),
                    causation_id: None,
                    trace_id: None,
                    payload: vistalith_domain::EventPayload::SubjectDefined(SubjectDefined {
                        subject: subject.clone(),
                        authority: AuthorityClass::Authoritative,
                        provenance: provenance.clone(),
                        properties: BTreeMap::new(),
                    }),
                },
                0,
            )
            .unwrap();
        }
        apply_event(
            &mut graph,
            &VEvent {
                event_id: uuid::Uuid::now_v7(),
                actor: actor("test:why"),
                timestamp: time::OffsetDateTime::now_utc(),
                subjects: vec![evidence.clone()],
                correlation_id: uuid::Uuid::now_v7(),
                causation_id: None,
                trace_id: None,
                payload: vistalith_domain::EventPayload::SubjectDefined(SubjectDefined {
                    subject: evidence.clone(),
                    authority: AuthorityClass::Derived,
                    provenance: provenance.clone(),
                    properties: BTreeMap::new(),
                }),
            },
            0,
        )
        .unwrap();

        let mut relate = |from: &SubjectRef, kind: RelationKind, to: &SubjectRef| {
            apply_event(
                &mut graph,
                &VEvent {
                    event_id: uuid::Uuid::now_v7(),
                    actor: actor("test:why"),
                    timestamp: time::OffsetDateTime::now_utc(),
                    subjects: Vec::new(),
                    correlation_id: uuid::Uuid::now_v7(),
                    causation_id: None,
                    trace_id: None,
                    payload: vistalith_domain::EventPayload::RelationDeclared(
                        RelationDeclared {
                            fact: RelationFact {
                                relation: RelationRef::new(from.clone(), kind, to.clone())
                                    .unwrap(),
                                authority: AuthorityClass::Authoritative,
                                provenance: provenance.clone(),
                            },
                        },
                    ),
                },
                0,
            )
            .unwrap();
        };
        relate(&evidence, RelationKind::ProvidesEvidenceFor, &decision);
        relate(&code, RelationKind::Implements, &decision);
        relate(&client, RelationKind::DependsOn, &code);
        graph
    }

    fn actor(raw: &'static str) -> vistalith_domain::Actor {
        vistalith_domain::Actor::new(raw).unwrap()
    }

    #[test]
    fn why_path_walks_support_backwards() {
        let graph = graph_with_chain();
        let decision = subject(SubjectKind::Container, "decision-svc");
        let path = why_path(&graph, &decision, 3).unwrap();

        let pairs: Vec<(u8, &str)> = path
            .links
            .iter()
            .map(|l| (l.depth, l.kind.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![(1, "implements"), (1, "provides_evidence_for"), (2, "depends_on")]
        );
        // Evidence backbone: only the hard-support edge.
        assert_eq!(path.evidence.len(), 1);
        assert_eq!(path.evidence[0].kind, "provides_evidence_for");
        assert_eq!(path.max_depth_reached, 2);
    }

    #[test]
    fn why_path_respects_max_depth_and_unknown_subjects() {
        let graph = graph_with_chain();
        let decision = subject(SubjectKind::Container, "decision-svc");
        let path = why_path(&graph, &decision, 1).unwrap();
        assert_eq!(path.links.len(), 2);
        assert_eq!(path.max_depth_reached, 1);

        let ghost = subject(SubjectKind::Container, "ghost");
        assert!(why_path(&graph, &ghost, 3).is_none());
    }
}
