# Architecture Emergence

## Ratchet E0
Plain domain + direct SDDK + in-memory graph.

## E1
Add event replay only after state/recovery UAT requires it.

## E2
Add SurrealDB only after graph semantics exist and storage spike passes.

## E3
Freeze Behavior API after 3 real behaviors including one relation behavior.

## E4
Freeze Lens SDK after 3 heterogeneous renderers.

## E5
Plugin/capability bundle system only after repeated extension patterns.

## E6
Remote/collaboration only after local identity/revision/conflict semantics mature.

## Re-analysis triggers
- graph >100k subjects hurts interactive latency;
- behavior cascades exceed budget;
- graph/source truth divergence;
- SDDK upgrade causes broad semantic breakage;
- context selection cannot explain itself;
- SurrealDB migration or binary footprint becomes painful;
- visual/agent models require duplicated domain concepts.
