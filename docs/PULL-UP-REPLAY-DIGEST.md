# Innovation Review — Deterministic Replay Digest

> Completed `governance/INNOVATION-REVIEW-TEMPLATE.md` for Vistalith's first
> pull-up candidate (slice 14, milestone M10). Submitted through the SDDK
> capability gateway via `vistalith-sddk-bridge` (`POST /sddk/pull-up`).

## Feature
Name: **deterministic-replay-digest**

## Vistalith problem solved
Every materialized view (SWG, lenses, reports) must be provably equal to the
strict projection of the durable event log. Without a canonical fingerprint,
"the graph is the projection" (B6) is an untestable claim.

## Semantic core
A SHA-256 digest over the canonical serialization of a projected state, plus
a strict replay algorithm that rebuilds the state from an append-only event
log and verifies stored revisions. UI-, LLM- and provider-free.

## SDDK focus test
- useful without GUI? **yes** — replay/digest runs in CLI, tests and agents.
- useful without LLM? **yes** — pure deterministic computation.
- relevant to workflow/decision/evidence/policy/knowledge/verification? **yes**
  (verification: digests detect corruption and divergence in the project
  ledger's projections).
- avoids duplicated semantic authority? **yes** — the log stays the sole
  authority; the digest verifies, never replaces.
- deterministic or explicitly uncertainty-aware? **yes** — same log always
  yields the same digest (enforced by tests).

## Evidence
UATs: `replay_tests::fixture_replays_deterministically`,
`graph_is_reconstructible_from_durable_log`,
`rebuild_tests::stored_log_rebuilds_to_same_digest`,
`workflow_sync_projects_cycles_and_is_idempotent` (replay determinism over
synced logs).
Metrics: any divergence between live projection and replay produces a
different digest — enforced continuously across 116+ tests.
Pain observed: pre-digest, a corrupted fixture was silently accepted; the
digest turns that class of bug into a test failure.

## Classification
**SDDK_PROPOSAL** (all focus criteria pass, evidence attached, horizon
proposed).

## Proposed SDDK horizon
**H4 — verification**: replay + digest as a verification primitive over any
append-only ledger projection.

## Rejected mechanics
Vistalith's `SemanticWorldGraph` type, the SWG subject vocabulary and the
axum surface do NOT travel with the proposal. SDDK would adopt the pattern
(canonical serialization → digest → strict rebuild verification) against its
own `LedgerEvent` stream and its own projection types — no Vistalith crate
imports.
