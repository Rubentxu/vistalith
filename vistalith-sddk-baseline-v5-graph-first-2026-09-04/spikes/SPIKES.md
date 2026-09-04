# Spikes

## SPK-001 SDDK direct core
Prove direct crate usage and lifecycle integration.

## SPK-002 ActiveGraph parity concepts
Implement a tiny Rust fixture with:
objects + relations + event log + behavior + relation behavior + patch + replay
+ fork/diff. Compare developer experience with ActiveGraph's mental model.

## SPK-003 SurrealDB embedded
Measure startup, size, durability, traversal, 100k/1m edges, rebuild and migration.

## SPK-004 petgraph
Benchmark traversal/impact/SCC/path operations on extracted subgraphs.

## SPK-005 Reactive patterns
Implement 5 real graph patterns. Reject a generic query DSL if typed predicates
are sufficient.

## SPK-006 Rig
Provider streaming/tool/structured-output parity across at least two providers
and one local/OpenAI-compatible endpoint.

## SPK-007 rmcp
stdio + Streamable HTTP + auth + tools changed + reconnect.

## SPK-008 LikeC4
Round-trip C4 node ↔ SubjectRef and architecture revision diff.

## SPK-009 Excalidraw
Persist semantic bindings independently from shape IDs.

## SPK-010 React Flow/ELK
1k/10k nodes, off-main-thread layout, live status updates.

## SPK-011 Fork/diff cache
Explore whether recorded model/tool outputs can make shared branch prefix cheap
without pretending live calls are deterministic.

## SPK-012 Graph → SDDK pull-up
Test SemanticChangeProposal against real SDDK Decision Plane requirements before
proposing any SDDK change.
