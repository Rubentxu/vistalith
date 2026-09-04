# Graph Storage Decision Strategy

Do not select a graph DB merely because the product is graph-first.

The graph model is domain truth; storage is replaceable infrastructure.

## Candidate A — SurrealDB embedded
Strong initial fit:
- Rust embedding;
- local-first;
- document + graph;
- relation records with edge data;
- graph traversal;
- future remote mode if ever needed.

## Candidate B — SQLite/event tables + in-memory petgraph
Fallback/reference baseline:
- maximum operational simplicity;
- explicit materialization;
- harder graph query ergonomics.

## Decision gate
SurrealDB wins only if the spike demonstrates:
- deterministic migrations/rebuild;
- acceptable binary/startup footprint;
- strong local durability;
- good traversal latency;
- no unacceptable lock-in at domain boundary.
