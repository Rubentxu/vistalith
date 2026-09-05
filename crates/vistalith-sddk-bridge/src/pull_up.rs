//! Innovation pull-up evaluation (M10, `sddk-evolution/INNOVATION-PULL-UP.md`
//! + `governance/INNOVATION-REVIEW-TEMPLATE.md`).
//!
//! A Vistalith feature can be *pulled up* into SDDK only through SDDK's own
//! decision plane. This module evaluates the innovation focus test —
//! deterministic, code-readable answers, no LLM —, classifies the feature
//! (`VISTALITH_ONLY` … `SDDK_PROPOSAL`) and, when classified
//! `SDDK_PROPOSAL`, submits the evaluation as governed evidence through the
//! SDDK capability gateway. No Vistalith mechanics (SWG types, lenses,
//! providers) enter the proposal: only the semantic core, the answers and
//! the evidence references.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use vistalith_domain::{Actor, EventPayload, SubjectRef};

use crate::event;
use vistalith_graph::GraphStore;

use crate::SddkBridge;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FocusAnswer {
    /// The criterion holds with evidence.
    Yes,
    /// The criterion does not hold.
    No,
}

/// The SDDK focus test (`INNOVATION-REVIEW-TEMPLATE.md`), as explicit
/// answers so the classification is reproducible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FocusTest {
    /// Useful without a GUI?
    pub gui_free: FocusAnswer,
    /// Useful without an LLM?
    pub llm_free: FocusAnswer,
    /// Relevant to workflow/decision/evidence/policy/knowledge/
    /// verification?
    pub semantic_relevance: FocusAnswer,
    /// Avoids duplicating semantic authority?
    pub no_duplicated_authority: FocusAnswer,
    /// Deterministic, or explicitly uncertainty-aware?
    pub deterministic: FocusAnswer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PullUpClassification {
    VistalithOnly,
    SddkWatch,
    SddkSpikeCandidate,
    SddkProposal,
}

/// A pull-up evaluation for one named feature.
#[derive(Debug, Clone)]
pub struct PullUpEvaluation {
    pub feature: String,
    /// The semantic core: what remains if UI/LLM/provider specifics are
    /// removed (one paragraph).
    pub semantic_core: String,
    pub focus_test: FocusTest,
    /// Evidence references (test names, UAT identifiers).
    pub evidence: Vec<String>,
    /// Proposed location in SDDK's existing H0-H12 (only for PROPOSAL).
    pub proposed_horizon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PullUpError {
    #[error("focus test answers `{failed:?}` fail the SDDK focus test")]
    FocusTestFailed { failed: Vec<&'static str> },
    #[error("this classification requires evidence references")]
    MissingEvidence,
    #[error("this classification does not propose a SDDK horizon; remove `proposed_horizon`")]
    HorizonNotAllowed,
    #[error("classification SDDK_PROPOSAL requires `proposed_horizon` (existing H0-H12)")]
    HorizonRequired,
    #[error("evaluation classifies as {actual:?}, not the desired classification")]
    ClassifiedOtherwise {
        actual: PullUpClassification,
    },
}

impl PullUpEvaluation {
    /// Deterministic classification from the focus test
    /// (`sddk-evolution/INNOVATION-PULL-UP.md` questions 1-6):
    ///
    /// - any failed criterion → `VISTALITH_ONLY` (the feature is UI/LLM
    ///   shaped, or duplicates authority, or is non-deterministic);
    /// - everything passes but no evidence attached → `SDDK_WATCH`
    ///   (plausible, unproven);
    /// - passing + evidence but no concrete horizon → `SDDK_SPIKE_CANDIDATE`
    ///   (needs a spike inside SDDK before a formal proposal);
    /// - passing + evidence + proposed horizon → `SDDK_PROPOSAL`.
    pub fn classify(&self) -> Result<PullUpClassification, PullUpError> {
        let checks: [(&'static str, bool); 5] = [
            ("gui-free", self.focus_test.gui_free == FocusAnswer::Yes),
            ("llm-free", self.focus_test.llm_free == FocusAnswer::Yes),
            (
                "semantic-relevance",
                self.focus_test.semantic_relevance == FocusAnswer::Yes,
            ),
            (
                "no-duplicated-authority",
                self.focus_test.no_duplicated_authority == FocusAnswer::Yes,
            ),
            (
                "deterministic",
                self.focus_test.deterministic == FocusAnswer::Yes,
            ),
        ];
        // Failing criteria is not an error: VISTALITH_ONLY *is* the
        // classification for features shaped by GUI/LLM or non-deterministic.
        let any_failed = checks.iter().any(|(_, ok)| !ok);
        if any_failed {
            return Ok(PullUpClassification::VistalithOnly);
        }
        if self.evidence.is_empty() {
            return Ok(PullUpClassification::SddkWatch);
        }
        match &self.proposed_horizon {
            Some(horizon) if !horizon.trim().is_empty() => {
                Ok(PullUpClassification::SddkProposal)
            }
            _ => Ok(PullUpClassification::SddkSpikeCandidate),
        }
    }

    /// Validates the evaluation against its own classification.
    pub fn validate(&self) -> Result<PullUpClassification, PullUpError> {
        let classification = self.classify()?;
        match classification {
            PullUpClassification::SddkProposal
            | PullUpClassification::SddkSpikeCandidate => {
                if self.evidence.is_empty() {
                    // Unreachable through classify() (no evidence → Watch)
                    // but kept as the single source of the invariant.
                    return Err(PullUpError::MissingEvidence);
                }
                if classification == PullUpClassification::SddkProposal
                    && self.proposed_horizon.as_ref().is_none_or(|h| h.trim().is_empty())
                {
                    return Err(PullUpError::HorizonRequired);
                }
            }
            _ => {}
        }
        Ok(classification)
    }

    /// Declares the intended classification and checks the evaluation
    /// satisfies its requirements — how a caller proposes "this should be a
    /// SDDK_PROPOSAL" and gets a precise rejection when it is not.
    pub fn validate_for(
        &self,
        desired: PullUpClassification,
    ) -> Result<PullUpClassification, PullUpError> {
        match desired {
            PullUpClassification::SddkProposal => {
                if self.evidence.is_empty() {
                    return Err(PullUpError::MissingEvidence);
                }
                if self
                    .proposed_horizon
                    .as_ref()
                    .is_none_or(|h| h.trim().is_empty())
                {
                    return Err(PullUpError::HorizonRequired);
                }
            }
            PullUpClassification::SddkSpikeCandidate => {
                if self.evidence.is_empty() {
                    return Err(PullUpError::MissingEvidence);
                }
                if self.proposed_horizon.is_some() {
                    return Err(PullUpError::HorizonNotAllowed);
                }
            }
            _ => {}
        }
        let actual = self.classify()?;
        if actual != desired {
            return Err(PullUpError::ClassifiedOtherwise { actual });
        }
        Ok(actual)
    }
}

/// Classification outcome, serialized into the governed evidence bundle.
#[derive(Debug, Clone, Serialize)]
pub struct PullUpOutcome {
    pub feature: String,
    pub classification: PullUpClassification,
    pub focus_test: FocusTest,
    pub semantic_core: String,
    pub evidence: Vec<String>,
    pub proposed_horizon: Option<String>,
    /// SDDK receipt id when the proposal was accepted by the gateway.
    pub receipt_id: Option<String>,
}

impl SddkBridge {
    /// Evaluates and, when the classification reaches `SDDK_PROPOSAL`,
    /// submits the evaluation as governed evidence through the SDDK
    /// capability gateway (the `sddk-proposal-submitted` event carries the
    /// receipt). Lower classifications are durable observations too: the
    /// evaluation itself is never lost.
    pub fn evaluate_pull_up(
        &self,
        store: &mut GraphStore,
        intent: &SubjectRef,
        target: &SubjectRef,
        evaluation: &PullUpEvaluation,
        actor: &Actor,
        approve: bool,
    ) -> Result<PullUpOutcome, crate::BridgeError> {
        let classification = evaluation
            .validate()
            .map_err(|e| crate::BridgeError::Serialization(e.to_string()))?;
        let outcome = PullUpOutcome {
            feature: evaluation.feature.clone(),
            classification,
            focus_test: evaluation.focus_test.clone(),
            semantic_core: evaluation.semantic_core.clone(),
            evidence: evaluation.evidence.clone(),
            proposed_horizon: evaluation.proposed_horizon.clone(),
            receipt_id: None,
        };
        // The evidence artifact references the evaluation content by digest:
        // the receipt in the SDDK ledger then verifiably captures this
        // pull-up evaluation.
        let content = outcome_to_bundle_content(&outcome);
        let content_json = serde_json::to_string(&content)
            .map_err(|e| crate::BridgeError::Serialization(e.to_string()))?;
        let mut hasher = Sha256::new();
        hasher.update(content_json.as_bytes());
        let digest = format!("sha256:{:x}", hasher.finalize());
        // Non-proposal classifications never reach the SDDK gateway: a
        // VISTALITH_ONLY feature must not propose itself to SDDK. The
        // evaluation is still durable as a Vistalith observation.
        let classification = evaluation
            .validate()
            .map_err(|e| crate::BridgeError::Serialization(e.to_string()))?;
        if classification != PullUpClassification::SddkProposal {
            let proposal = SubjectRef::new(
                vistalith_domain::Namespace::Vistalith,
                vistalith_domain::SubjectKind::Proposal,
                uuid::Uuid::now_v7().to_string(),
            )
            .expect("generated proposal id is valid");
            if store.graph().subject(target).is_none() {
                store
                    .append(event(
                        actor.clone(),
                        EventPayload::SubjectDefined(vistalith_domain::SubjectDefined {
                            subject: target.clone(),
                            authority: vistalith_domain::AuthorityClass::Derived,
                            provenance: vistalith_domain::Provenance::new("system:pull-up")
                                .expect("static provenance"),
                            properties: std::collections::BTreeMap::from([(
                                "project_id".to_owned(),
                                serde_json::json!(self.project_id()),
                            )]),
                        }),
                        vec![target.clone()],
                    ))
                    .map_err(|e| crate::BridgeError::Serialization(e.to_string()))?;
            }
            store
                .append(event(
                    actor.clone(),
                    EventPayload::SddkProposalSubmitted(crate::SddkProposalSubmitted {
                        proposal: proposal.clone(),
                        intent: intent.clone(),
                        target: target.clone(),
                        capability: "evidence.write".to_owned(),
                        decision: "not-proposed".to_owned(),
                        receipt_id: None,
                        receipt: serde_json::json!({
                            "classification": classification,
                            "reason": "classification below SDDK_PROPOSAL",
                        }),
                    }),
                    vec![proposal.clone(), intent.clone(), target.clone()],
                ))
                .map_err(|e| crate::BridgeError::Serialization(e.to_string()))?;
            return Ok(PullUpOutcome {
                classification,
                receipt_id: None,
                ..outcome
            });
        }

        let bundle = serde_json::json!({
            "artifacts": [{
                "kind": "note",
                "ref": digest,
                "path": format!("pull-up/{}.json", evaluation.feature),
                "mime": "application/json",
                "note": content_json,
            }],
            "environment": {},
            "execution": {},
        });
        // The evaluation target must exist in the graph; the observed SDDK
        // project subject is a legitimate derived default.
        if store.graph().subject(target).is_none() {
            let mut properties = std::collections::BTreeMap::new();
            properties.insert(
                "project_id".to_owned(),
                serde_json::json!(self.project_id()),
            );
            properties
                .insert("source".to_owned(), serde_json::json!("pull-up evaluation"));
            store
                .append(event(
                    actor.clone(),
                    EventPayload::SubjectDefined(vistalith_domain::SubjectDefined {
                        subject: target.clone(),
                        authority: vistalith_domain::AuthorityClass::Derived,
                        provenance: vistalith_domain::Provenance::new("system:pull-up")
                            .expect("static provenance"),
                        properties,
                    }),
                    vec![target.clone()],
                ))
                .map_err(|e| crate::BridgeError::Serialization(e.to_string()))?;
        }
        let proposal = self.submit_evidence_proposal(
            store,
            intent,
            target,
            bundle,
            format!("pull-up evaluation: {}", evaluation.feature),
            actor,
            approve,
        )?;

        // Read the durable decision back to complete the outcome.
        let node = store
            .graph()
            .subject(&proposal)
            .ok_or_else(|| {
                crate::BridgeError::Serialization(
                    "bridge proposal missing from projection".to_owned(),
                )
            })?;
        let receipt_id = node
            .properties
            .get("receipt_id")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        Ok(PullUpOutcome { receipt_id, ..outcome })
    }
}

fn outcome_to_bundle_content(outcome: &PullUpOutcome) -> serde_json::Value {
    serde_json::json!({
        "feature": outcome.feature,
        "semantic_core": outcome.semantic_core,
        "focus_test": outcome.focus_test,
        "evidence": outcome.evidence,
        "classification": outcome.classification,
        "proposed_horizon": outcome.proposed_horizon,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing(feature: &str, horizon: Option<String>) -> PullUpEvaluation {
        PullUpEvaluation {
            feature: feature.to_owned(),
            semantic_core: "deterministic replay digest over an append-only log".to_owned(),
            focus_test: FocusTest {
                gui_free: FocusAnswer::Yes,
                llm_free: FocusAnswer::Yes,
                semantic_relevance: FocusAnswer::Yes,
                no_duplicated_authority: FocusAnswer::Yes,
                deterministic: FocusAnswer::Yes,
            },
            evidence: vec![
                "replay_tests::fixture_replays_deterministically".to_owned(),
                "rebuild_tests::stored_log_rebuilds_to_same_digest".to_owned(),
            ],
            proposed_horizon: horizon,
        }
    }

    #[test]
    fn passing_test_with_horizon_classifies_as_proposal() {
        let evaluation = passing("replay-digest", Some("H4 verification".to_owned()));
        assert_eq!(
            evaluation.classify().unwrap(),
            PullUpClassification::SddkProposal
        );
        assert_eq!(evaluation.validate().unwrap(), PullUpClassification::SddkProposal);
    }

    #[test]
    fn passing_without_evidence_is_watch_and_declared_spike_needs_evidence() {
        let mut watch = passing("replay-digest", None);
        watch.evidence.clear();
        assert_eq!(
            watch.classify().unwrap(),
            PullUpClassification::SddkWatch
        );
        // Declaring a spike without evidence is rejected precisely.
        let mut spike = passing("replay-digest", None);
        spike.evidence.clear();
        assert!(matches!(
            spike.validate_for(PullUpClassification::SddkSpikeCandidate),
            Err(PullUpError::MissingEvidence)
        ));
        // A real spike candidate has evidence but no horizon.
        let mut spike = passing("replay-digest", None);
        spike.proposed_horizon = None;
        assert_eq!(
            spike
                .validate_for(PullUpClassification::SddkSpikeCandidate)
                .unwrap(),
            PullUpClassification::SddkSpikeCandidate
        );
    }

    #[test]
    fn failed_criteria_classify_vistalith_only() {
        let mut evaluation = passing("ui-lens", Some("H9".to_owned()));
        evaluation.focus_test.gui_free = FocusAnswer::No;
        evaluation.focus_test.llm_free = FocusAnswer::No;
        assert_eq!(
            evaluation.classify().unwrap(),
            PullUpClassification::VistalithOnly
        );
        // And declaring PROPOSAL for it fails with a classified-otherwise
        // rejection, not a silent pass.
        assert!(matches!(
            evaluation.validate_for(PullUpClassification::SddkProposal),
            Err(PullUpError::ClassifiedOtherwise { .. })
        ));
    }

    #[test]
    fn declaring_proposal_without_horizon_is_rejected() {
        let mut evaluation = passing("replay-digest", None);
        evaluation.proposed_horizon = None;
        // Without a horizon the evaluation itself classifies as a spike
        // candidate...
        assert_eq!(
            evaluation.classify().unwrap(),
            PullUpClassification::SddkSpikeCandidate
        );
        // ...but declaring PROPOSAL fails with the precise requirement.
        assert!(matches!(
            evaluation.validate_for(PullUpClassification::SddkProposal),
            Err(PullUpError::HorizonRequired)
        ));
    }
}
