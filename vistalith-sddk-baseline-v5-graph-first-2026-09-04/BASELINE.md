# Baseline decisions

## B1 — SDDK is the core
Vistalith directly consumes existing SDDK Rust crates. No internal network/process
boundary is invented.

## B2 — SDDK remains agnostic
No Vistalith-specific chat, LLM, Rig, MCP, renderer, protocol or client library
is added to SDDK.

## B3 — Vistalith agentic runtime is Rust
Provider/model runtime, context assembly, MCP, tool execution orchestration,
conversation persistence, tracing and agent loops live in Vistalith Rust crates.

## B4 — TypeScript owns human experience
React/TypeScript owns chat rendering, model/MCP control surfaces and visual lenses.

## B5 — graph-first
Vistalith models engineering knowledge as a typed semantic graph with provenance,
revision and authority metadata.

## B6 — event-first
Vistalith-owned state transitions emit durable events. Materialized graph views
are reconstructible from durable sources.

## B7 — Visual Intent
A visual gesture may create semantic intent; it never silently performs an
engineering effect.

## B8 — innovations can flow down into SDDK
When Vistalith discovers a generic software-development kernel capability, it is
evaluated for pull-up into SDDK using explicit focus/admission criteria.

## B9 — ActiveGraph is inspiration, not dependency
Adopt valuable primitives in Rust while preserving SDDK authority and avoiding
a second workflow/decision kernel.

## B10 — architecture evolves from UAT evidence
Do not create plugin systems, graph clusters, collaboration CRDTs or complex
distributed services before a measured need exists.
