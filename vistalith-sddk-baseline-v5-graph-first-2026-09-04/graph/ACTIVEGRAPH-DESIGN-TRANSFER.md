# ActiveGraph Design Transfer

ActiveGraph's core proposition is highly aligned with Vistalith:

> graph as world, behaviors as reactive logic, event trace as proof.

We adopt the ideas, not the Python runtime.

## Mapping

| ActiveGraph concept | Vistalith adaptation |
|---|---|
| Graph | Semantic World Graph |
| Objects | Semantic Subjects |
| Typed relations | Semantic Relations |
| Event log | Vistalith Event Log |
| Behaviors | Reactive Behaviors |
| Relation behaviors | Relation Behaviors |
| Patches | GraphChangeProposal / GraphPatch |
| Views | SemanticContextView |
| Frames | Agent/Exploration Frame |
| Policies | Vistalith permission + SDDK authority composition |
| Patterns | GraphPattern subscriptions |
| Replay | Vistalith replay |
| Forking | Exploration/Experiment Branch |
| Failure as event | RuntimeFailure event |
| Packs | Capability/Lens/Domain bundles, later if repeated need exists |

## Important adaptation

ActiveGraph can coordinate agentic behavior through the graph. Vistalith must not
use that idea to create a second SDDK workflow authority.

Reactive behaviors are initially limited to:
- projections;
- context maintenance;
- advisory proposals;
- notifications;
- Vistalith-owned chat/visual state;
- graph-derived suggestions.

If a behavior wants to cause SDDK-governed engineering work, it creates a proposal
that enters SDDK's normal decision/workflow/gateway path.

## High-value ideas

### Relation behaviors
Logic can live on relationships.

Example:
`implements(CodeSymbol, Component)` can react to a code-change event and mark an
architecture projection potentially stale.

### Fork-and-diff
Branch a semantic/agentic exploration, change assumptions/model/architecture
option, then compare resulting graph structure.

### Pattern subscriptions
React to shapes, not only event types:
- Requirement with no evidence;
- Decision with a revisit trigger that became true;
- Component changed while verifying tests are stale;
- Agent contribution contradicting accepted evidence.

### Patch lifecycle
`proposed → applied | rejected` is an excellent model for advisory graph changes
and Visual Intent.

### Failure as history
A failed model/tool/reactive behavior becomes traceable data, not invisible
exception-only control flow.
