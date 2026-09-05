//! Decision lens inventory (`visual/DECISIONS-TIME.md`, milestone M9):
//! every decision subject with its question, selected option, rejected
//! alternatives, motivating requirement and supporting evidence — all read
//! from the SWG's typed relations, nothing authoritatively mutated.

use serde::Serialize;
use vistalith_domain::{RelationKind, SubjectKind};

use crate::graph::SemanticWorldGraph;

#[derive(Debug, Clone, Serialize)]
pub struct DecisionAlternative {
    pub option: String,
    /// The edge direction: an option that was `rejected_in_favor_of` the
    /// decision lost; an option the decision was `decided_by`... etc.
    pub via: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecisionEntry {
    pub decision: String,
    /// The question this decision answers, when a `motivated_by` /
    /// `mentions` edge points at a question subject.
    pub question: Option<String>,
    /// The option that won (incoming `decided_by` from the selected
    /// option subject, when modeled that way).
    pub selected: Option<String>,
    /// Alternatives that lost, via `rejected_in_favor_of` edges.
    pub rejected: Vec<DecisionAlternative>,
    /// Requirements this decision is `motivated_by`.
    pub motivated_by: Vec<String>,
    /// Evidence supporting the decision (`provides_evidence_for`,
    /// `verifies`, `supports`).
    pub evidence: Vec<String>,
    /// Contradictions surfacing against this decision.
    pub contradicts: Vec<String>,
    /// Decisions this one revisits (`revisits`), with the identity of the
    /// superseded decision.
    pub revisits: Vec<String>,
    pub deprecated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecisionsLens {
    pub decisions: Vec<DecisionEntry>,
}

pub fn decisions_lens(graph: &SemanticWorldGraph) -> DecisionsLens {
    let mut decisions = Vec::new();
    for node in graph.subjects() {
        if node.subject.kind() != &SubjectKind::Decision {
            continue;
        }
        let decision = &node.subject;
        let mut entry = DecisionEntry {
            decision: decision.to_string(),
            question: None,
            selected: None,
            rejected: Vec::new(),
            motivated_by: Vec::new(),
            evidence: Vec::new(),
            contradicts: Vec::new(),
            revisits: Vec::new(),
            deprecated: node.deprecated,
        };

        // Outgoing motivation: `decision -[motivated_by]-> requirement`.
        for fact in graph.outgoing(decision) {
            if fact.relation.kind == RelationKind::MotivatedBy {
                entry.motivated_by.push(fact.relation.to.to_string());
            }
        }
        for fact in graph.incoming(decision) {
            let from = fact.relation.from.to_string();
            match fact.relation.kind {
                RelationKind::DecidedBy => entry.selected = Some(from),
                RelationKind::ProvidesEvidenceFor | RelationKind::Verifies => {
                    entry.evidence.push(from)
                }
                RelationKind::Contradicts => entry.contradicts.push(from),
                RelationKind::RejectedInFavorOf => entry.rejected.push(DecisionAlternative {
                    option: from,
                    via: "rejected_in_favor_of".to_owned(),
                }),
                RelationKind::Revisits => entry.revisits.push(from),
                RelationKind::Mentions => {
                    // A question subject mentioning the decision reads as
                    // the question it answers.
                    if let Some(question) = graph.subject(&fact.relation.from)
                        && question.subject.kind() == &SubjectKind::Question
                    {
                        entry.question = Some(from);
                    }
                }
                _ => {}
            }
        }
        // Outgoing rejected_in_favor_of: THIS decision lost to another —
        // modeled as an alternative it rejected in favor of the winner.
        for fact in graph.outgoing(decision) {
            if fact.relation.kind == RelationKind::RejectedInFavorOf {
                entry.rejected.push(DecisionAlternative {
                    option: decision.to_string(),
                    via: "rejected_in_favor_of (this decision lost)".to_owned(),
                });
            }
        }

        entry.evidence.sort();
        entry.motivated_by.sort();
        entry.contradicts.sort();
        entry.revisits.sort();
        decisions.push(entry);
    }
    decisions.sort_by(|a, b| a.decision.cmp(&b.decision));
    DecisionsLens { decisions }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::apply_event;
    use std::collections::BTreeMap;
    use vistalith_domain::{
        AuthorityClass, EventPayload, Namespace, Provenance, RelationDeclared, RelationFact,
        RelationRef, SubjectDefined, SubjectRef, VEvent,
    };

    fn arch(id: &str) -> SubjectRef {
        SubjectRef::new(Namespace::Arch, SubjectKind::Container, id.to_owned()).unwrap()
    }

    fn any(kind: SubjectKind, id: &str) -> SubjectRef {
        SubjectRef::new(Namespace::Vistalith, kind, id.to_owned()).unwrap()
    }

    fn event(payload: EventPayload) -> VEvent {
        VEvent {
            event_id: uuid::Uuid::now_v7(),
            actor: vistalith_domain::Actor::new("user:ruben").unwrap(),
            timestamp: time::OffsetDateTime::now_utc(),
            subjects: Vec::new(),
            correlation_id: uuid::Uuid::now_v7(),
            causation_id: None,
            trace_id: None,
            payload,
        }
    }

    fn define(graph: &mut SemanticWorldGraph, subject: SubjectRef) {
        apply_event(
            graph,
            &event(EventPayload::SubjectDefined(SubjectDefined {
                subject,
                authority: AuthorityClass::Authoritative,
                provenance: Provenance::new("user:ruben").unwrap(),
                properties: BTreeMap::new(),
            })),
            0,
        )
        .unwrap();
    }

    fn relate(graph: &mut SemanticWorldGraph, from: &SubjectRef, kind: RelationKind, to: &SubjectRef) {
        apply_event(
            graph,
            &event(EventPayload::RelationDeclared(RelationDeclared {
                fact: RelationFact {
                    relation: RelationRef::new(from.clone(), kind, to.clone()).unwrap(),
                    authority: AuthorityClass::Authoritative,
                    provenance: Provenance::new("user:ruben").unwrap(),
                },
            })),
            0,
        )
        .unwrap();
    }

    #[test]
    fn decision_lens_collects_question_options_and_evidence() {
        let mut graph = SemanticWorldGraph::new();
        let decision = any(SubjectKind::Decision, "d-1");
        let question = any(SubjectKind::Question, "q-1");
        let winner = any(SubjectKind::Option, "option-b");
        let loser = any(SubjectKind::Option, "option-a");
        let requirement = arch("req-payment");
        let evidence = SubjectRef::new(
            Namespace::Verification,
            SubjectKind::Evidence,
            "bench-1".to_owned(),
        )
        .unwrap();

        for subject in [&decision, &question, &winner, &loser, &requirement, &evidence] {
            define(&mut graph, subject.clone());
        }
        relate(&mut graph, &question, RelationKind::Mentions, &decision);
        relate(&mut graph, &winner, RelationKind::DecidedBy, &decision);
        relate(&mut graph, &loser, RelationKind::RejectedInFavorOf, &decision);
        relate(&mut graph, &decision, RelationKind::MotivatedBy, &requirement);
        relate(&mut graph, &evidence, RelationKind::ProvidesEvidenceFor, &decision);

        let lens = decisions_lens(&graph);
        assert_eq!(lens.decisions.len(), 1);
        let entry = &lens.decisions[0];
        assert_eq!(entry.decision, decision.to_string());
        assert_eq!(entry.question.as_deref(), Some(question.to_string().as_str()));
        assert_eq!(entry.selected.as_deref(), Some(winner.to_string().as_str()));
        assert_eq!(entry.rejected.len(), 1);
        assert_eq!(entry.rejected[0].option, loser.to_string());
        assert_eq!(entry.motivated_by, vec![requirement.to_string()]);
        assert_eq!(entry.evidence, vec![evidence.to_string()]);
        assert!(entry.contradicts.is_empty());
        assert!(!entry.deprecated);
    }

    #[test]
    fn contradictions_and_revisits_surface_per_decision() {
        let mut graph = SemanticWorldGraph::new();
        let decision = any(SubjectKind::Decision, "d-old");
        let newer = any(SubjectKind::Decision, "d-new");
        let contradicter = any(SubjectKind::Hypothesis, "h-1");
        define(&mut graph, decision.clone());
        define(&mut graph, newer.clone());
        define(&mut graph, contradicter.clone());
        relate(&mut graph, &newer, RelationKind::Revisits, &decision);
        relate(&mut graph, &contradicter, RelationKind::Contradicts, &decision);

        let lens = decisions_lens(&graph);
        let old = lens
            .decisions
            .iter()
            .find(|d| d.decision == decision.to_string())
            .unwrap();
        assert_eq!(old.revisits, vec![newer.to_string()]);
        assert_eq!(old.contradicts, vec![contradicter.to_string()]);
    }

    #[test]
    fn non_decision_subjects_are_ignored() {
        let mut graph = SemanticWorldGraph::new();
        define(&mut graph, arch("payment-service"));
        assert!(decisions_lens(&graph).decisions.is_empty());
    }
}
