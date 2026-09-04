# Graph Database Research Notes

## SurrealDB
Current stable 3.2.x line is attractive for Vistalith because it supports:
- Rust embedded deployment;
- memory or file-backed engines;
- first-class graph relations with data on edges;
- document and graph data in one engine;
- graph traversal;
- future live-query/remote options.

The choice is not yet frozen because Vistalith's event-sourced graph semantics
must remain independent from the storage engine.

## petgraph
`petgraph 0.8.x` is appropriate for in-memory graph algorithms and makes a useful
reference implementation for deterministic domain tests.

## Decision
Run the storage spike after the domain/event model exists. Do not write the
domain in SurrealQL.
