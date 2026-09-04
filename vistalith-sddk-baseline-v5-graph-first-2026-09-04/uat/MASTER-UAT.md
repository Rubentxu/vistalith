# Master UAT

## UAT-01 Direct SDDK core
No CLI/process/protocol indirection is used for core state.

## UAT-02 Graph identity
One semantic subject selected in Chat/C4/Workflow resolves to identical SubjectRef.

## UAT-03 Replay
Rebuild SWG from durable events/sources and compare semantic graph hash.

## UAT-04 Provenance
Every derived edge explains its origin.

## UAT-05 Authority
Advisory graph patch cannot mutate SDDK authoritative state.

## UAT-06 Relation behavior
A deterministic edge behavior reacts exactly once per qualifying event and
produces idempotent projected result.

## UAT-07 Failure event
Behavior failure is visible in trace and replay, not lost as log-only exception.

## UAT-08 Semantic context
Chat context for a subject is selected through graph relations and shows inclusion reasons.

## UAT-09 Provider fallback
Fallback is visible with reason and usage attribution.

## UAT-10 MCP policy
Side-effecting MCP tool cannot bypass permission/SDDK policy.

## UAT-11 Visual Intent
Drag/draw creates draft proposal only.

## UAT-12 Stale proposal
Base revision changes; intent becomes stale and requires re-preview.

## UAT-13 Fork/diff
Two advisory branches have deterministic structural diff.

## UAT-14 Architecture traceability
C4 component navigates to code, decision, tests and evidence.

## UAT-15 Workflow truth
Workflow lens matches SDDK runtime state without a duplicate authority store.

## UAT-16 UAT Studio
Human acceptance result is stored through SDDK UAT semantics.

## UAT-17 Time travel
Render graph at two historical revisions and explain changed edges.

## UAT-18 Innovation pull-up
A candidate cannot enter SDDK proposal state without focus/generalization/UAT evidence.
