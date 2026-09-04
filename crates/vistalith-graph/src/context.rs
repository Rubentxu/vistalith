//! Semantic Context View (SPEC-005, milestone M3): a bounded, explainable
//! slice of the SWG for LLM context assembly.
//!
//! Every bound in the spec is a request field — roots, relation allowlist,
//! depth, authority classes, recency, token budget — and every inclusion or
//! exclusion carries a provenance reason, so a caller (chat, agent, context
//! compiler) can explain *why* each item is present. This view is a pure
//! function of the store: no hidden state, deterministic for equal inputs
//! and equal logs.

use std::collections::{BTreeMap, HashSet, VecDeque};

use serde::Serialize;
use time::OffsetDateTime;
use vistalith_domain::{AuthorityClass, RelationKind, SubjectRef};

use crate::store::GraphStore;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextRequest {
    pub roots: Vec<SubjectRef>,
    /// Relation kinds the slice may traverse; `None` allows every kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relations: Option<Vec<RelationKind>>,
    /// Maximum hops from a root (roots are depth 0).
    #[serde(default = "default_max_depth")]
    pub max_depth: u8,
    #[serde(default)]
    pub include_derived: bool,
    #[serde(default)]
    pub include_advisory: bool,
    /// Recency bound: subjects whose last touch predates this cutoff are
    /// excluded (and reported as such). `None` disables the bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recency_cutoff: Option<OffsetDateTime>,
    /// Approximate token budget (≈ chars / 4, same estimator the offline
    /// provider uses). The view never exceeds it.
    #[serde(default = "default_budget")]
    pub token_budget: usize,
}

fn default_max_depth() -> u8 {
    2
}

fn default_budget() -> usize {
    8_000
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum InclusionReason {
    /// Explicitly requested as a root.
    Root,
    /// Reached through a relation.
    Via { from: String, kind: String, depth: u8 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum ExclusionReason {
    UnknownSubject,
    LastTouchedBeforeCutoff { last_touch: String },
    AuthorityClass { class: String },
    DeeperThanMaxDepth { depth: u8 },
    TokenBudgetExhausted,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextItem {
    pub subject: String,
    pub authority: AuthorityClass,
    pub depth: u8,
    pub reason: InclusionReason,
    pub properties: BTreeMap<String, serde_json::Value>,
    /// Provenance of the subject's last touch: log position, time and actor.
    pub last_event_sequence: u64,
    pub last_touch: String,
    pub last_actor: String,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextExclusion {
    pub subject: String,
    pub exclusion: ExclusionReason,
}

#[derive(Debug, Clone, Serialize)]
pub struct SemanticContextView {
    pub roots: Vec<String>,
    pub items: Vec<ContextItem>,
    /// Subjects discovered but not included, each with its reason — negative
    /// knowledge is part of the provenance (PATTERNS-VIEWS-FRAMES.md).
    pub exclusions: Vec<ContextExclusion>,
    pub estimated_tokens: usize,
    pub token_budget: usize,
    pub truncated: bool,
}

/// Approximates tokens as chars / 4 — the same estimator the offline
/// provider uses. Deterministic, good enough for budgeting.
fn estimate_tokens(value: &serde_json::Value) -> usize {
    value.to_string().len().div_ceil(4)
}

fn subject_json(
    subject: &SubjectRef,
    properties: &BTreeMap<String, serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "subject": subject.to_string(),
        "properties": properties,
    })
}

fn authority_allowed(request: &ContextRequest, authority: AuthorityClass) -> bool {
    match authority {
        AuthorityClass::Authoritative => true,
        AuthorityClass::Derived => request.include_derived,
        AuthorityClass::Advisory => request.include_advisory,
        // Ephemeral facts never belong in a durable context slice.
        AuthorityClass::Ephemeral => false,
    }
}

pub fn build_context_view(store: &GraphStore, request: &ContextRequest) -> SemanticContextView {
    let graph = store.graph();
    let log = store.log();
    let last_touch = |sequence: u64| log[sequence as usize].event.timestamp;
    let last_actor = |sequence: u64| log[sequence as usize].event.actor.to_string();

    let mut view = SemanticContextView {
        roots: request.roots.iter().map(|r| r.to_string()).collect(),
        items: Vec::new(),
        exclusions: Vec::new(),
        estimated_tokens: 0,
        token_budget: request.token_budget,
        truncated: false,
    };
    let mut visited: HashSet<SubjectRef> = HashSet::new();
    let mut queue: VecDeque<(SubjectRef, u8, Option<InclusionReason>)> = VecDeque::new();
    let mut excluded: HashSet<String> = HashSet::new();

    let record_exclusion = |view: &mut SemanticContextView,
                                excluded: &mut HashSet<String>,
                                subject: &SubjectRef,
                                reason: ExclusionReason| {
        if excluded.insert(subject.to_string()) {
            view.exclusions.push(ContextExclusion {
                subject: subject.to_string(),
                exclusion: reason,
            });
        }
    };

    for root in &request.roots {
        queue.push_back((
            root.clone(),
            0,
            Some(InclusionReason::Root),
        ));
    }

    while let Some((subject, depth, reason)) = queue.pop_front() {
        if !visited.insert(subject.clone()) {
            continue;
        }
        let Some(node) = graph.subject(&subject) else {
            record_exclusion(
                &mut view,
                &mut excluded,
                &subject,
                ExclusionReason::UnknownSubject,
            );
            continue;
        };
        if !authority_allowed(request, node.authority) {
            record_exclusion(
                &mut view,
                &mut excluded,
                &subject,
                ExclusionReason::AuthorityClass {
                    class: serde_json::to_value(node.authority)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default(),
                },
            );
            continue;
        }
        if let Some(cutoff) = request.recency_cutoff {
            let touch = last_touch(node.last_event_sequence);
            if touch < cutoff {
                record_exclusion(
                    &mut view,
                    &mut excluded,
                    &subject,
                    ExclusionReason::LastTouchedBeforeCutoff {
                        last_touch: touch
                            .format(&time::format_description::well_known::Rfc3339)
                            .unwrap_or_default(),
                    },
                );
                continue;
            }
        }
        let json = subject_json(&subject, &node.properties);
        let tokens = estimate_tokens(&json);
        if view.estimated_tokens + tokens > request.token_budget {
            view.truncated = true;
            record_exclusion(
                &mut view,
                &mut excluded,
                &subject,
                ExclusionReason::TokenBudgetExhausted,
            );
            continue;
        }

        view.estimated_tokens += tokens;
        view.items.push(ContextItem {
            subject: subject.to_string(),
            authority: node.authority,
            depth,
            reason: reason.unwrap_or(InclusionReason::Root),
            properties: node.properties.clone(),
            last_event_sequence: node.last_event_sequence,
            last_touch: last_touch(node.last_event_sequence)
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            last_actor: last_actor(node.last_event_sequence),
            estimated_tokens: tokens,
        });

        if depth >= request.max_depth {
            for fact in graph.outgoing(&subject) {
                if let Some(allowed) = &request.relations
                    && !allowed.contains(&fact.relation.kind)
                {
                    continue;
                }
                if !visited.contains(&fact.relation.to) {
                    record_exclusion(
                        &mut view,
                        &mut excluded,
                        &fact.relation.to,
                        ExclusionReason::DeeperThanMaxDepth { depth: depth + 1 },
                    );
                }
            }
            continue;
        }

        for fact in graph.outgoing(&subject) {
            if let Some(allowed) = &request.relations
                && !allowed.contains(&fact.relation.kind)
            {
                continue;
            }
            if !visited.contains(&fact.relation.to) {
                queue.push_back((
                    fact.relation.to.clone(),
                    depth + 1,
                    Some(InclusionReason::Via {
                        from: subject.to_string(),
                        kind: fact.relation.kind.as_str().to_owned(),
                        depth: depth + 1,
                    }),
                ));
            }
        }
    }

    view
}
