# Vistalith + SDDK — Baseline v5 Graph-First

**Date:** 2026-09-04  
**Status:** proposed consolidated baseline  
**Supersedes:** all previous Vistalith/SDDK architecture packs produced in this design sequence.

This pack intentionally contains **no superseded SddkPort, SDDK app-server,
@sddk/client or Vistalith-specific SDDK SDK architecture**.

## Final relationship

```text
SDDK = deterministic software-development decision kernel
Vistalith = agentic + visual engineering product built directly on SDDK
```

Dependency direction:

```text
SDDK Rust crates
       ▲
       │ direct compile-time dependency
       │
Vistalith Rust backend
       ▲
       │ Vistalith protocol
       │
Web / Desktop / Terminal clients
```

SDDK remains independent and agnostic. It never imports Vistalith, Rig, React,
MCP, model-provider concepts or visual-renderer concepts merely because Vistalith
needs them.

Vistalith may use SDDK deeply and directly because **SDDK is its engineering
kernel**.

## Product thesis

Vistalith is not "an IDE with chat". It is a **Visual Agentic Engineering
Environment** where chat, code, architecture, decisions, evidence, workflows,
agents and free-form visual thinking are different lenses over shared semantic
subjects.

The graph is the working model. The trace explains what happened. SDDK supplies
governed engineering truth and effects.

## ActiveGraph influence

ActiveGraph is a major conceptual reference, not a runtime dependency.

The design borrows and adapts:
- event-sourced graph thinking;
- typed objects and relations;
- reactive behaviors;
- behavior on relations;
- proposed/applied/rejected patches;
- scoped graph views;
- frames;
- policies;
- graph-pattern reactions;
- replay;
- fork-and-diff;
- failures as events;
- composable packs.

The implementation target is Rust-native and aligned with SDDK's authority model.

Start at `START-HERE.md`.
