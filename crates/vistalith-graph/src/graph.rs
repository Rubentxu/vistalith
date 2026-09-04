use std::collections::BTreeMap;

use serde::Serialize;
use vistalith_domain::{AuthorityClass, Namespace, Provenance, SubjectRef};

/// A materialized subject in the Semantic World Graph.
///
/// Every fact carries authority, provenance and an event cursor
/// (`graph/SEMANTIC-WORLD-GRAPH.md`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SubjectNode {
    pub subject: SubjectRef,
    pub authority: AuthorityClass,
    pub provenance: Provenance,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub deprecated: bool,
    /// Log position of the last event that touched this node.
    pub last_event_sequence: u64,
}

impl SubjectNode {
    /// True when the node holds SDDK-owned authoritative truth: such nodes
    /// are never mutated by Vistalith graph patches (SPEC-001 invariant 4).
    pub fn is_sddk_owned(&self) -> bool {
        self.subject.namespace() == &Namespace::Sddk && self.authority.is_authoritative()
    }
}

/// The Semantic World Graph: typed subjects and relations with a monotonically
/// increasing revision. Ordered containers keep iteration deterministic, which
/// is what makes digests and snapshots stable across processes.
#[derive(Debug, Clone, Default)]
pub struct SemanticWorldGraph {
    subjects: BTreeMap<SubjectRef, SubjectNode>,
    relations: BTreeMap<vistalith_domain::RelationRef, vistalith_domain::RelationFact>,
    revision: u64,
}

impl SemanticWorldGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Revision of the graph: bumped once per state-changing event.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn subject(&self, subject: &SubjectRef) -> Option<&SubjectNode> {
        self.subjects.get(subject)
    }

    pub fn subjects(&self) -> impl Iterator<Item = &SubjectNode> {
        self.subjects.values()
    }

    pub fn subjects_of_kind(
        &self,
        kind: &vistalith_domain::SubjectKind,
    ) -> impl Iterator<Item = &SubjectNode> {
        self.subjects
            .values()
            .filter(move |n| n.subject.kind() == kind)
    }

    pub fn relation(
        &self,
        relation: &vistalith_domain::RelationRef,
    ) -> Option<&vistalith_domain::RelationFact> {
        self.relations.get(relation)
    }

    pub fn relations(&self) -> impl Iterator<Item = &vistalith_domain::RelationFact> {
        self.relations.values()
    }

    pub fn relations_of_kind(
        &self,
        kind: &vistalith_domain::RelationKind,
    ) -> impl Iterator<Item = &vistalith_domain::RelationFact> {
        self.relations
            .values()
            .filter(move |f| &f.relation.kind == kind)
    }

    /// Relations leaving `from`, ordered by `(kind, to)`.
    pub fn outgoing(
        &self,
        from: &SubjectRef,
    ) -> impl Iterator<Item = &vistalith_domain::RelationFact> {
        self.relations
            .values()
            .filter(move |f| &f.relation.from == from)
    }

    /// Relations entering `to`, ordered by `(from, kind)`.
    pub fn incoming(
        &self,
        to: &SubjectRef,
    ) -> impl Iterator<Item = &vistalith_domain::RelationFact> {
        self.relations
            .values()
            .filter(move |f| &f.relation.to == to)
    }

    /// Subjects reachable from `parent` through `contains` edges, ordered by
    /// identity. Conversation threads use this to reconstruct their items.
    pub fn children(&self, parent: &SubjectRef) -> Vec<&SubjectNode> {
        self.outgoing(parent)
            .filter(|f| f.relation.kind == vistalith_domain::RelationKind::Contains)
            .filter_map(|f| self.subjects.get(&f.relation.to))
            .collect()
    }

    pub fn subject_count(&self) -> usize {
        self.subjects.len()
    }

    pub fn relation_count(&self) -> usize {
        self.relations.len()
    }

    // --- Mutation primitives (used by the event projection) ---

    /// Inserts a subject or, when it already exists (patch `UpsertSubject`
    /// on Vistalith-owned nodes), merges properties only: authority and
    /// provenance of the first definition are never overwritten.
    pub(crate) fn upsert_subject(
        &mut self,
        subject: SubjectRef,
        authority: AuthorityClass,
        provenance: Provenance,
        properties: BTreeMap<String, serde_json::Value>,
        sequence: u64,
    ) {
        match self.subjects.get_mut(&subject) {
            Some(node) => {
                node.properties.extend(properties);
                node.last_event_sequence = sequence;
            }
            None => {
                let node = SubjectNode {
                    subject: subject.clone(),
                    authority,
                    provenance,
                    properties,
                    deprecated: false,
                    last_event_sequence: sequence,
                };
                self.subjects.insert(subject, node);
            }
        }
    }

    /// Merges properties into an existing subject; returns `false` when the
    /// subject does not exist.
    pub(crate) fn update_subject(
        &mut self,
        subject: &SubjectRef,
        properties: &BTreeMap<String, serde_json::Value>,
        sequence: u64,
    ) -> bool {
        match self.subjects.get_mut(subject) {
            Some(node) => {
                node.properties.extend(properties.clone());
                node.last_event_sequence = sequence;
                true
            }
            None => false,
        }
    }

    pub(crate) fn deprecate_subject(&mut self, subject: &SubjectRef, sequence: u64) -> bool {
        match self.subjects.get_mut(subject) {
            Some(node) => {
                node.deprecated = true;
                node.last_event_sequence = sequence;
                true
            }
            None => false,
        }
    }

    /// Inserts a relation; returns `false` when the exact relation already exists.
    pub(crate) fn declare_relation(
        &mut self,
        fact: vistalith_domain::RelationFact,
        sequence: u64,
    ) -> bool {
        if self.relations.contains_key(&fact.relation) {
            return false;
        }
        if let Some(node) = self.subjects.get_mut(&fact.relation.from) {
            node.last_event_sequence = sequence;
        }
        if let Some(node) = self.subjects.get_mut(&fact.relation.to) {
            node.last_event_sequence = sequence;
        }
        self.relations.insert(fact.relation.clone(), fact);
        true
    }

    pub(crate) fn relation_endpoint_exists(
        &self,
        relation: &vistalith_domain::RelationRef,
    ) -> bool {
        self.subjects.contains_key(&relation.from) && self.subjects.contains_key(&relation.to)
    }

    pub(crate) fn bump_revision(&mut self) -> u64 {
        self.revision += 1;
        self.revision
    }

    pub(crate) fn node(&self, subject: &SubjectRef) -> Option<&SubjectNode> {
        self.subjects.get(subject)
    }
}
