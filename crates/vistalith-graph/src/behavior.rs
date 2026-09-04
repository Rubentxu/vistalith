//! Reactive behaviors (SPEC-003, `graph/REACTIVE-BEHAVIORS.md`, milestone M4).
//!
//! A behavior declares what it subscribes to (event types and/or relation
//! kinds) and reacts to a durable event by **emitting events** — advisory
//! ones, never hidden side effects. The store appends those outputs with
//! `causation_id` pointing at the triggering event, so every advisory is
//! traceable to its cause in the log itself.
//!
//! Guardrail (SPEC-003 / ActiveGraph transfer): a behavior cannot silently
//! turn advisory state into authoritative SDDK state. Structurally enforced:
//! the only payload a behavior may emit is [`EventPayload::AdvisoryRaised`],
//! whose projection creates advisory-class subjects; anything else is
//! rejected at dispatch.
//!
//! Behaviors run on **live appends only**. Replay does not re-run them —
//! their outputs are already durable events in the log — which keeps replay
//! byte-deterministic (determinism class: `DeterministicRule`).

use vistalith_domain::{
    AdvisoryRaised, DeterminismClass, EventPayload, Namespace, RelationKind, SubjectKind,
    SubjectRef, VEvent,
};

use crate::graph::SemanticWorldGraph;

/// Declarative contract of a behavior (`graph/REACTIVE-BEHAVIORS.md`).
#[derive(Debug, Clone)]
pub struct BehaviorSpec {
    /// Behavior identity, e.g. `impact-advisory`. Emitted events carry
    /// `behavior: "<name>@<version>"`.
    pub name: String,
    pub version: u32,
    /// Event types (`VEvent::kind()` values) that activate the behavior.
    pub subscribes: &'static [&'static str],
    /// Relation kinds the behavior is attached to (RelationBehavior contract:
    /// activation must identify the relation that caused it — the reacting
    /// behavior receives the declaring event and inspects the relation).
    pub relation_kinds: &'static [RelationKind],
    /// Deterministic rules only participate in live dispatch (v1).
    pub determinism: DeterminismClass,
}

impl BehaviorSpec {
    pub fn identity(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }

}

/// Everything a behavior may look at when reacting: the projected graph and
/// the triggering event.
pub struct BehaviorContext<'a> {
    pub graph: &'a SemanticWorldGraph,
    pub trigger: &'a VEvent,
}

/// A reactive behavior. Implementations are pure: they read the context and
/// return advisory payloads; the store appends them durably.
pub trait Behavior: Send + Sync {
    fn spec(&self) -> BehaviorSpec;
    fn react(&self, ctx: &BehaviorContext) -> Vec<EventPayload>;
}

/// Errors raised while dispatching behavior outputs.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BehaviorError {
    #[error("behavior `{0}` tried to emit a non-advisory payload: behaviors may only raise advisories (SPEC-003 guardrail)")]
    NonAdvisoryOutput(String),
}

/// Log coordinates of an append plus how many advisories the behaviors
/// raised for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendedWithAdvisories {
    pub appended: crate::store::AppendedEvent,
    pub advisories: u64,
}

/// Appends `event`, then dispatches subscribed behaviors and appends their
/// advisory outputs with full trace (`correlation_id` inherited from the
/// trigger, `causation_id` = the trigger's event id).
pub fn append_and_react(
    store: &mut crate::store::GraphStore,
    event: vistalith_domain::VEvent,
    behaviors: &[Box<dyn Behavior>],
) -> Result<AppendedWithAdvisories, crate::store::StoreError> {
    use vistalith_domain::Actor;
    use vistalith_domain::EventPayload;
    let trigger_id = event.event_id;
    let correlation_id = event.correlation_id;
    let trigger_kind = event.kind().to_owned();
    let appended = store.append(event)?;
    let mut raised = 0u64;
    for behavior in behaviors {
        let spec = behavior.spec();
        if !spec.subscribes.contains(&trigger_kind.as_str()) {
            continue;
        }
        let ctx = BehaviorContext {
            graph: store.graph(),
            // The behaviors see the trigger as it entered the log.
            trigger: &store.log()[appended.sequence as usize].event,
        };
        let identity = spec.identity();
        for payload in behavior.react(&ctx) {
            let EventPayload::AdvisoryRaised(advisory) = &payload else {
                return Err(crate::store::StoreError::BehaviorOutputRejected(
                    BehaviorError::NonAdvisoryOutput(identity.clone()),
                ));
            };
            let actor = Actor::new(format!("behavior:{}", identity))
                .map_err(|e| crate::store::StoreError::Serialization(e.to_string()))?;
            let output = VEvent {
                event_id: uuid::Uuid::now_v7(),
                actor,
                timestamp: time::OffsetDateTime::now_utc(),
                subjects: vec![advisory.advisory.clone(), advisory.about.clone()],
                correlation_id,
                causation_id: Some(trigger_id),
                trace_id: Some(format!("behavior:{}", identity)),
                payload,
            };
            store.append(output)?;
            raised += 1;
        }
    }
    Ok(AppendedWithAdvisories { appended, advisories: raised })
}

// --- Built-in behaviors (SPK-005: typed predicates, not a query DSL) --------

/// RelationBehavior on `depends_on`: when a dependency changes, every direct
/// dependent gets an advisory impact note ("X depends_on Y; Y changed").
pub struct ImpactAdvisory;

impl Behavior for ImpactAdvisory {
    fn spec(&self) -> BehaviorSpec {
        BehaviorSpec {
            name: "impact-advisory".to_owned(),
            version: 1,
            subscribes: &["subject-updated", "subject-deprecated"],
            relation_kinds: &[RelationKind::DependsOn],
            determinism: DeterminismClass::DeterministicRule,
        }
    }

    fn react(&self, ctx: &BehaviorContext) -> Vec<EventPayload> {
        let changed = match &ctx.trigger.payload {
            EventPayload::SubjectUpdated(u) => &u.subject,
            EventPayload::SubjectDeprecated(d) => &d.subject,
            _ => return Vec::new(),
        };
        let note_body = match &ctx.trigger.payload {
            EventPayload::SubjectDeprecated(_) => "was deprecated",
            _ => "changed",
        };
        ctx.graph
            .incoming(changed)
            .filter(|fact| fact.relation.kind == RelationKind::DependsOn)
            .map(|fact| {
                EventPayload::AdvisoryRaised(AdvisoryRaised {
                    advisory: advisory_ref("impact"),
                    about: fact.relation.from.clone(),
                    behavior: "impact-advisory@1".to_owned(),
                    note: format!(
                        "`{}` depends_on `{}`, which {note_body}.",
                        fact.relation.from,
                        fact.relation.to
                    ),
                })
            })
            .collect()
    }
}

/// RelationBehavior on `contradicts`: a contradiction between two subjects
/// surfaces as an advisory naming both sides.
pub struct ContradictionAdvisory;

impl Behavior for ContradictionAdvisory {
    fn spec(&self) -> BehaviorSpec {
        BehaviorSpec {
            name: "contradiction-advisory".to_owned(),
            version: 1,
            subscribes: &["relation-declared"],
            relation_kinds: &[RelationKind::Contradicts],
            determinism: DeterminismClass::DeterministicRule,
        }
    }

    fn react(&self, ctx: &BehaviorContext) -> Vec<EventPayload> {
        let EventPayload::RelationDeclared(declared) = &ctx.trigger.payload else {
            return Vec::new();
        };
        let relation = &declared.fact.relation;
        if relation.kind != RelationKind::Contradicts {
            return Vec::new();
        }
        vec![EventPayload::AdvisoryRaised(AdvisoryRaised {
            advisory: advisory_ref("contradiction"),
            about: relation.from.clone(),
            behavior: "contradiction-advisory@1".to_owned(),
            note: format!(
                "`{}` contradicts `{}` — surface the conflict.",
                relation.from, relation.to
            ),
        })]
    }
}

/// Pattern: evidence subject deprecated while it supports something → the
/// supported subject gets a stale-evidence advisory.
pub struct StaleEvidenceAdvisory;

impl Behavior for StaleEvidenceAdvisory {
    fn spec(&self) -> BehaviorSpec {
        BehaviorSpec {
            name: "stale-evidence-advisory".to_owned(),
            version: 1,
            subscribes: &["subject-deprecated"],
            relation_kinds: &[RelationKind::ProvidesEvidenceFor, RelationKind::Verifies],
            determinism: DeterminismClass::DeterministicRule,
        }
    }

    fn react(&self, ctx: &BehaviorContext) -> Vec<EventPayload> {
        let EventPayload::SubjectDeprecated(deprecated) = &ctx.trigger.payload else {
            return Vec::new();
        };
        ctx.graph
            .outgoing(&deprecated.subject)
            .filter(|fact| {
                matches!(
                    fact.relation.kind,
                    RelationKind::ProvidesEvidenceFor | RelationKind::Verifies
                )
            })
            .map(|fact| {
                EventPayload::AdvisoryRaised(AdvisoryRaised {
                    advisory: advisory_ref("stale-evidence"),
                    about: fact.relation.to.clone(),
                    behavior: "stale-evidence-advisory@1".to_owned(),
                    note: format!(
                        "evidence `{}` was deprecated; `{}` rests on stale evidence.",
                        fact.relation.from, fact.relation.to
                    ),
                })
            })
            .collect()
    }
}

/// Pattern: an SDDK work item observed without any evidence edge gets a
/// one-time "lacks evidence" advisory on definition.
pub struct MissingEvidenceAdvisory;

impl Behavior for MissingEvidenceAdvisory {
    fn spec(&self) -> BehaviorSpec {
        BehaviorSpec {
            name: "missing-evidence-advisory".to_owned(),
            version: 1,
            subscribes: &["subject-defined"],
            relation_kinds: &[],
            determinism: DeterminismClass::DeterministicRule,
        }
    }

    fn react(&self, ctx: &BehaviorContext) -> Vec<EventPayload> {
        let EventPayload::SubjectDefined(defined) = &ctx.trigger.payload else {
            return Vec::new();
        };
        let is_work_item = ctx.graph.subject(&defined.subject).is_some_and(|node| {
            node.subject.namespace() == &Namespace::Sddk
                && node.subject.kind() == &SubjectKind::WorkItem
        });
        if !is_work_item {
            return Vec::new();
        }
        let has_evidence = ctx
            .graph
            .incoming(&defined.subject)
            .any(|fact| fact.relation.kind == RelationKind::ProvidesEvidenceFor);
        if has_evidence {
            return Vec::new();
        }
        vec![EventPayload::AdvisoryRaised(AdvisoryRaised {
            advisory: advisory_ref("missing-evidence"),
            about: defined.subject.clone(),
            behavior: "missing-evidence-advisory@1".to_owned(),
            note: format!(
                "work item `{}` has no providing_evidence_for incoming edge.",
                defined.subject
            ),
        })]
    }
}

/// The built-in set, boxed and ordered (dispatch order is the array order).
pub fn builtin_behaviors() -> Vec<Box<dyn Behavior>> {
    vec![
        Box::new(ImpactAdvisory),
        Box::new(ContradictionAdvisory),
        Box::new(StaleEvidenceAdvisory),
        Box::new(MissingEvidenceAdvisory),
    ]
}

fn advisory_ref(seed: &str) -> SubjectRef {
    SubjectRef::new(
        Namespace::Vistalith,
        SubjectKind::Advisory,
        format!("{seed}-{}", uuid::Uuid::now_v7()),
    )
    .expect("generated advisory id is valid")
}
