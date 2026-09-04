//! Structural graph diff (SPEC-011): added/removed/changed subjects and
//! relations between two graph revisions. Changed subjects report
//! property-level detail; changed relations report both fact versions
//! (authority and provenance differences included, so confidence/evidence
//! drift is visible). Diffs are read-only projections — they never touch
//! SDDK truth and never execute anything.

use serde::Serialize;
use vistalith_domain::{RelationFact, RelationRef, SubjectRef};

use crate::graph::SemanticWorldGraph;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PropertyChange {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SubjectChange {
    pub subject: SubjectRef,
    pub changes: Vec<PropertyChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RelationChange {
    pub relation: RelationRef,
    pub from: RelationFact,
    pub to: RelationFact,
}

/// Structural diff of two graph states. All vectors are ordered by identity,
/// so the same pair of revisions always yields the same diff — a deterministic
/// projection like everything else in the graph crate.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphDiff {
    pub added_subjects: Vec<SubjectRef>,
    pub removed_subjects: Vec<SubjectRef>,
    pub changed_subjects: Vec<SubjectChange>,
    pub added_relations: Vec<RelationRef>,
    pub removed_relations: Vec<RelationRef>,
    pub changed_relations: Vec<RelationChange>,
}

impl GraphDiff {
    pub fn is_empty(&self) -> bool {
        self.added_subjects.is_empty()
            && self.removed_subjects.is_empty()
            && self.changed_subjects.is_empty()
            && self.added_relations.is_empty()
            && self.removed_relations.is_empty()
            && self.changed_relations.is_empty()
    }
}

pub fn diff_graphs(from: &SemanticWorldGraph, to: &SemanticWorldGraph) -> GraphDiff {
    let mut added_subjects = Vec::new();
    let mut changed_subjects = Vec::new();
    for to_node in to.subjects() {
        let Some(from_node) = from.subject(&to_node.subject) else {
            added_subjects.push(to_node.subject.clone());
            continue;
        };
        let mut changes = property_changes(&from_node.properties, &to_node.properties);
        if from_node.deprecated != to_node.deprecated {
            // Deprecation is a first-class fact, not a property: report it
            // as a change from absent to present (or back).
            changes.insert(
                0,
                PropertyChange {
                    key: "deprecated".to_owned(),
                    from: from_node.deprecated.then_some(serde_json::json!(true)),
                    to: to_node.deprecated.then_some(serde_json::json!(true)),
                },
            );
        }
        if !changes.is_empty() {
            changed_subjects.push(SubjectChange {
                subject: to_node.subject.clone(),
                changes,
            });
        }
    }
    let removed_subjects: Vec<SubjectRef> = from
        .subjects()
        .filter(|node| to.subject(&node.subject).is_none())
        .map(|node| node.subject.clone())
        .collect();

    let mut added_relations = Vec::new();
    let mut changed_relations = Vec::new();
    for to_fact in to.relations() {
        match from.relation(&to_fact.relation) {
            None => added_relations.push(to_fact.relation.clone()),
            Some(from_fact) if from_fact != to_fact => {
                changed_relations.push(RelationChange {
                    relation: to_fact.relation.clone(),
                    from: from_fact.clone(),
                    to: to_fact.clone(),
                });
            }
            Some(_) => {}
        }
    }
    let removed_relations: Vec<RelationRef> = from
        .relations()
        .filter(|fact| to.relation(&fact.relation).is_none())
        .map(|fact| fact.relation.clone())
        .collect();

    GraphDiff {
        added_subjects,
        removed_subjects,
        changed_subjects,
        added_relations,
        removed_relations,
        changed_relations,
    }
}

fn property_changes(
    from: &std::collections::BTreeMap<String, serde_json::Value>,
    to: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Vec<PropertyChange> {
    let mut changes = Vec::new();
    for (key, to_value) in to {
        match from.get(key) {
            None => changes.push(PropertyChange {
                key: key.clone(),
                from: None,
                to: Some(to_value.clone()),
            }),
            Some(from_value) if from_value != to_value => changes.push(PropertyChange {
                key: key.clone(),
                from: Some(from_value.clone()),
                to: Some(to_value.clone()),
            }),
            Some(_) => {}
        }
    }
    for (key, from_value) in from {
        if !to.contains_key(key) {
            changes.push(PropertyChange {
                key: key.clone(),
                from: Some(from_value.clone()),
                to: None,
            });
        }
    }
    changes
}
