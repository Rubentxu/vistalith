# ActiveGraph Research Notes

Reference project:
`yoheinakajima/activegraph`

The current project describes itself as an event-sourced reactive graph runtime
for long-running auditable agentic systems. Its strongest reusable ideas for
Vistalith are architectural, not language-specific.

## Concepts worth carrying forward

### Event-sourced graph
Objects + typed relations are projected from an append-only trace.

### Behaviors
Logic reacts to events rather than requiring every component to call every other
component directly.

### Relation behaviors
Coordination can live on an edge. This is particularly attractive for software
semantics such as `implements`, `depends_on`, `verified_by` and `blocks`.

### Patches
Changes can be proposed and rejected with optimistic concurrency rather than
mutating shared state invisibly.

### Views
A behavior should receive only the graph slice needed for its job.

### Frames
Goal + constraints + budget + available behaviors/tools form a bounded context.

### Patterns
A behavior can react to a semantic shape rather than only one event type.

### Replay
The trace can reconstruct state and test deterministic behavior.

### Fork-and-diff
A branch at an event can explore another assumption and compare structural
outcomes against the parent.

### Failure as event
Failures remain in history and can participate in observability/recovery.

### Packs
Composable domain bundles are valuable, but Vistalith should only add a pack
system after repeated extension patterns emerge.

## Important differences

Vistalith:
- is Rust-first in the backend;
- embeds SDDK as its engineering kernel;
- cannot let reactive graph behaviors become a parallel SDDK workflow authority;
- has richer visual/editor interaction concerns;
- has a graph spanning code/architecture/chat/LLM/SDDK domains.

## Concrete adoption target

Create a minimal Rust-native parity spike:
- subject;
- relation;
- event;
- behavior;
- relation behavior;
- graph patch;
- context view;
- pattern;
- replay;
- fork/diff.

Use that spike to decide which primitives deserve production status.
