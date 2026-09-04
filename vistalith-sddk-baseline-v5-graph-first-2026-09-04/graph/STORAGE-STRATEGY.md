# Graph Storage Strategy

## Preferred spike baseline

### SurrealDB embedded
Evaluate SurrealDB 3.2.x stable line as local graph persistence:
- embedded Rust;
- file-based engines;
- graph relations as first-class records;
- graph traversal;
- live-query capabilities;
- document + graph model in one engine.

### petgraph
Use `petgraph 0.8.x` for in-memory algorithms where query/persistence is not the
right abstraction:
- traversal;
- SCC/topological operations;
- path algorithms;
- graph transformations;
- test fixtures.

## Event log

Do not assume the graph database itself is the event-sourcing model.

Persist explicit append-only Vistalith events and build graph projections.

## Spike questions

1. Can SurrealDB embedded rebuild 100k/1m relation projections quickly?
2. Are graph traversals fast enough for interactive lenses?
3. Does embedded file durability meet desktop needs?
4. Can event replay be made deterministic?
5. Can schema migrations be tested reproducibly?
6. Does binary size/startup cost remain acceptable?
7. Can project-per-database isolation be simple?
8. Does live-query behavior help enough to justify complexity?

## Fallback

If SurrealDB fails the spike, retain graph-first domain semantics and substitute
storage. The domain must not become SurrealQL-shaped.
