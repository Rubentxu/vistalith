# Vistalith

[English](README.md) | [Español](README.es.md)

**Vistalith is an agentic, visual engineering workspace whose core is a
Semantic World Graph (SWG), built directly on top of the
[SDDK](https://github.com/Rubentxu/software-development-decision-kernel)
crates.** SDDK remains the authority for planning, workflows, decisions and
evidence; Vistalith adds the agentic interaction plane (conversations,
providers, tools), the visual workspace and the cross-domain semantic graph
that ties engineering knowledge together.

This README is **normative**: the rules below govern how this repository is
built and changed. The full planning baseline lives in
[`vistalith-sddk-baseline-v5-graph-first-2026-09-04/`](vistalith-sddk-baseline-v5-graph-first-2026-09-04/START-HERE.md)
(normative reading order in its `START-HERE.md`).

## Status

| Slice | Scope | State |
|---|---|---|
| 1 | Rust workspace, SDDK pin, `SubjectRef`, `VEvent`, in-memory SWG, deterministic replay, `vistalithd` | done |
| 2 | `@vistalith/client` + web graph lens with cross-lens `SubjectRef` selection | done |
| 3 | Conversation threads, one provider through Rig, C4 projection | done |
| 4 | Native tool (`graph_search`) + VisualIntent draft/preview/promote lifecycle | done |
| 5 | SurrealDB spike (gated — **gate closed**, `docs/SURREALDB-SPIKE.md`), thread fork + graph diff/time travel (SPEC-011), Tauri desktop shell | done |

## Normative baseline decisions

| # | Rule |
|---|---|
| B1 | **SDDK is the core.** Vistalith consumes SDDK Rust crates directly; no internal network/process boundary and no `SddkPort` façade is invented. Compile errors from SDDK upgrades are evidence of real coupling, not something to hide. |
| B2 | **SDDK stays agnostic.** No Vistalith-specific chat, LLM, Rig, MCP, renderer or client code is pulled up into SDDK. |
| B3 | **The agentic runtime is Rust.** Providers, context assembly, MCP, tool orchestration, conversation persistence and tracing live in Vistalith Rust crates. |
| B4 | **TypeScript owns human experience.** Chat rendering, control surfaces and visual lenses are React/TypeScript. |
| B5 | **Graph-first.** Engineering knowledge is a typed semantic graph with provenance, revision and authority metadata. |
| B6 | **Event-first.** Vistalith state transitions emit durable events; every materialized view is reconstructible from the durable log. |
| B7 | **Visual Intent.** A visual gesture may create semantic intent; it never silently performs an engineering effect. |
| B8 | **Innovations may flow down into SDDK** through explicit pull-up evaluation. |
| B9 | **ActiveGraph is inspiration, not dependency.** |
| B10 | **Architecture emerges from UAT evidence.** No plugin marketplaces, CRDT collaboration, graph clustering or distributed services before a measured need. |

## Authority split

**SDDK owns:** planning and work-item truth, workflow/run state, legal next
actions, policy/gateway, evidence and receipts, decision memory, deterministic
lifecycle.

**Vistalith adds:** conversations, providers/models, Rig, MCP, agent
interaction runtime, the visual workspace, the cross-domain Semantic World
Graph, LLM usage tracing, the client protocol and rendering lenses.

**Rule:** Vistalith never reimplements a capability merely to avoid depending
on SDDK — if the capability belongs to SDDK, call SDDK directly.

## Hard invariants (enforced in code and tests)

1. Renderer node IDs are never semantic IDs; selection propagates
   `SubjectRef`s (`namespace:kind:id`, revision-aware but revision excluded
   from identity) across every lens.
2. SDDK-owned subjects are never authoritatively mutated by a Vistalith graph
   patch: such patches are rejected (`must-be-governed-by-sddk`) and must
   become governed SDDK semantic proposals. Vistalith holds SDDK truth as
   *derived observations* with provenance.
3. Graph patches carry a base revision (optimistic concurrency); stale bases
   are rejected, and rejections are durable events.
4. The graph is a projection: the event log is the durable source of truth,
   replay is deterministic (SHA-256 graph digest) and rebuilds are verified
   against stored revisions.
5. Every graph fact carries source, source revision, authority class and
   provenance; advisory facts are distinguishable.
6. Forks (SPEC-011) are advisory exploration state: a forked thread copies
   its items with `forked_of` bindings back to the originals and a
   `forked_from` relation to its source; time travel (`graph?at_revision=R`)
   is a strict replay of the log prefix, and structural diffs are
   deterministic. Promotion into SDDK stays explicit and governed.

## Repository layout

```text
crates/
├── vistalith-domain         # SubjectRef, VEvent, patch types, authority classes
├── vistalith-graph          # in-memory SWG, event projection, patches, C4 view, replay, diff
├── vistalith-agent-runtime  # conversation engine + provider contracts (Rig behind them)
├── vistalith-server         # `vistalithd` — axum server over the event log + SWG
└── vistalith-spike-surrealdb  # SPK-003 storage gate spike (own toolchain; excluded)
packages/
└── client             # @vistalith/client — TS protocol mirror + typed HTTP client
apps/
├── web                # React/Vite graph lens (subjects/edges, SubjectRef selection)
└── desktop            # Tauri 2 shell wrapping the web lens + vistalithd lifecycle
dev/                   # pinned SDDK checkout + pinned sddk CLI binary (gitignored)
docs/DEPENDENCIES.md   # dependency pins and pin policy
docs/SURREALDB-SPIKE.md  # SPK-003 gate report and verdict
vistalith-sddk-baseline-v5-graph-first-2026-09-04/  # planning baseline (docs)
```

## Dependency pinning policy

- **Toolchain:** Rust 1.91.0 (`rust-toolchain.toml`), Node ≥ 24, pnpm 12
  (`packageManager`), exact pins in manifests and lockfiles
  (`.npmrc` → `save-exact=true`, committed `Cargo.lock` / `pnpm-lock.yaml`).
- **SDDK** is pinned to one exact tag/commit, materialized in an intermediate
  machine-local checkout:

  ```bash
  scripts/bootstrap-dev.sh                 # clone + checkout the pinned revision
  scripts/bootstrap-dev.sh --pin v1.83.0   # move the pin (updates scripts/sddk-pin.env)
  ```

  All consumed SDDK crates resolve to that single revision; never mix
  revisions. Only pin refs that exist on the SDDK origin. The pinned `sddk`
  CLI binary lives in `dev/bin/` with a SHA-256 manifest. An SDDK upgrade is a
  first-class dependency upgrade: update pin → compile → contract/graph
  projection tests → master UAT → semantic diff → accept or revert.

## Building and running

```bash
# Rust core + server
cargo test
cargo run -p vistalith-server --bin vistalithd \
  --fixture crates/vistalith-graph/tests/fixtures/sample-world.json --port 7420

# TypeScript workspace
pnpm install
pnpm build && pnpm test && pnpm lint
pnpm dev:web        # http://localhost:5173 → talks to vistalithd on :7420

# Desktop shell (Tauri 2; wraps the same web lens, can launch vistalithd)
pnpm install
pnpm desktop:dev    # WebKit/GTK devel headers required — see scripts/tauri-env.sh

# SurrealDB storage spike (SPK-003; isolated: nightly toolchain, own lockfile)
cd crates/vistalith-spike-surrealdb
cargo test --features file-engine
cargo run --release --features file-engine -- --engine surrealkv --nodes 50000
```

`vistalithd` API: `GET /health`, `GET /graph` (optional `?at_revision=R`
time travel), `GET /diff?from=A[&to=B]` (structural graph diff),
`GET /subjects`, `GET /subjects/{namespace}/{kind}/{id}`, `GET|POST /events`,
`POST /patches` (applied → `200`, rejected → `409`; rejections are durable
events), `POST|GET /threads`, `GET /threads/{id}`,
`POST /threads/{id}/messages` (one provider turn per message),
`POST /threads/{id}/fork` (SPEC-011: copy items up to a turn with
`forked_of` bindings, link the fork back with `forked_from`),
`POST|GET /intents`, `GET /intents/{id}`, `POST /intents/{id}/promote`,
`POST /intents/{id}/discard` (SPEC-006 lifecycle) and `GET /views/c4`.

The web client has three lenses over the same identities: **Graph**
(subjects/edges, with a time-travel selector and structural diff when
viewing a past revision), **C4** (projected view) and **Chat** (threads,
with a per-thread fork action; copied items are marked `⎇ forked`).
Selecting a subject in any lens propagates the same `SubjectRef`.

## Storage decision (SPK-003)

The SurrealDB spike ran the full gate from `technology/GRAPH-STORAGE-DECISION.md`
and **the gate stays closed**: surrealdb 3.x (both the baseline's 3.2.x line
and 3.1.6) does not compile with any stable Rust toolchain this project uses
(its exact-pinned `diskann` dependency trips rust-lang/rust#100013); on
nightly the engine measured well (deterministic rebuild, 3-hop traversal
p95 0.35 ms at 1M relations, durable reopen, digest parity with the SWG
projection) but adopting it would fork the toolchain. Storage remains
**Candidate B**: durable JSON event log + strict in-memory projection.
Full evidence and reproduction: [`docs/SURREALDB-SPIKE.md`](docs/SURREALDB-SPIKE.md).

Providers: `--provider fake` (offline, default) or `--provider anthropic
--model claude-haiku-4-5` with `VISTALITH_ANTHROPIC_API_KEY` (read once,
never returned to any renderer — SPEC-008). `VITE_VISTALITHD_URL` points the
web client elsewhere.

## License

[MIT](LICENSE)
