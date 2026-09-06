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
| 6 | MCP client (rmcp, stdio + Streamable HTTP), unified tool catalog with scoped permission grants (SPEC-009, TOOLS-PERMISSIONS) | done |
| 7 | Reactive behaviors (SPEC-003), graph algorithms via petgraph (ADR-007), semantic context view (SPEC-005) | done |
| 8 | Frames — bounded execution contexts — plus Vistalith agents and delegation (`PATTERNS-VIEWS-FRAMES.md`, `AGENTS-DELEGATION.md`) | done |
| 9 | Governed SDDK promotion bridge — intents on SDDK-owned subjects go through the SDDK capability gateway, receipts durable (SPK-012, M7) | done |
| 10 | SDDK workflow projection into the SWG (M6) + why-path traceability (M9) | done |
| 11 | Streaming turns — SSE deltas to the web chat, identical durability (SPK-006 partial) | done |
| 12 | MCP server model completion — health, auto-reconnect, tools re-discovery, enable/disable (SPK-007 partial) | done |
| 13 | Decision lens — question/options/rejected/evidence inventory per decision (M9, DECISIONS-TIME.md) | done |
| 14 | Innovation pull-up — focus-test evaluation + governed submission to SDDK (M10, INNOVATION-PULL-UP.md) | done |
| 15 | UAT checks — durable pass/fail/blocked records per scenario with lens inventory (UAT-STUDIO.md) | done |
| 16 | Full impact analysis — direct/transitive, tests, stale evidence, invalidated decisions, explicit unknown impact (visual/IMPACT.md) | done |
| 17 | Thinking canvas — free-form primitives as advisory subjects + progressive formalization to VisualIntent (VISUAL-THINKING.md) | done |
| 18 | Agent runs — agent-defined frames, structured outputs, contributes_to/executed_by traceability (AGENTS-DELEGATION.md) | done |
| 19 | LikeC4 round-trip — C4 DSL export/import with `SubjectRef` identity in metadata + architecture revision diff (SPK-008) | done |

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
7. Tools (native and MCP) project into one catalog (SPEC-009). Permission
   outcomes are deny / allow / ask: read-only tools run free, write-class
   tools need a scoped temporary grant (per-call, consumable, revocable),
   explicit denies always win. Every call — granted or refused — is a
   durable `ToolInvoked` event carrying the tool's source. Vistalith
   permissions restrict; they never weaken SDDK policy.
8. Reactive behaviors (SPEC-003) emit advisory events only — never hidden
   side effects, never authoritative SDDK state (structurally enforced:
   the only payload a behavior may emit is `advisory-raised`). Advisories
   are durable, advisory-class subjects traced to their trigger via
   `causation_id`; replay does not re-run behaviors, so replay stays
   byte-deterministic.
9. Frames are bounded execution contexts: a frame owns a thread, its
   permitted tools restrict the unified catalog (bounds never weaken the
   permission gate), and its turn/token budgets are durable accounting
   that closes the frame automatically. Closed frames refuse further
   turns; every bound and outcome is an event.
10. SDDK promotion is governed end to end (SPK-012): with the bridge
   configured, promoting an intent on an SDDK-owned subject submits a
   `Proposal` through SDDK's `CapabilityGateway` (default-deny policy from
   the project workflow; high-risk capabilities demand explicit approval).
   The decision and the SDDK receipt are durable in **both** ledgers —
   SDDK's (the receipt) and Vistalith's (a `sddk-proposal-submitted` event
   projected as a derived observation providing evidence for the target).
   Without the bridge, the legacy governance routing applies.
11. SDDK workflow projection (M6) and the why-path (M9) are read-only
   observations: sync materializes ledger cycles as derived
   `sddk:workflow:<id>` subjects (idempotent, deterministic event ids),
   and the why-path only follows incoming support edges — neither ever
   writes SDDK state.
12. Streaming is transport-only (SPK-006): deltas may reach the UI as they
   stream, but durability never changes — the same events append at the
   same points, and the terminal streamed event carries the aggregated
   response exactly like a non-streamed completion.
13. UAT checks (UAT-STUDIO.md) are durable Vistalith facts about a
   scenario: verdict (pass/fail/blocked), optional evidence reference and
   note, inventoried per scenario with the latest verdict. The GUI never
   defines a parallel UAT lifecycle — where the scenario is
   SDDK-governed, SDDK semantics remain authoritative.
14. The LikeC4 round-trip (SPK-008) is identity-preserving: the C4 DSL
   export carries every element's `SubjectRef` in
   `metadata { vistalith "ns:kind:id" }`, and re-importing an untouched
   export appends nothing (report: unchanged/skipped, graph revision
   unchanged). Foreign models (no metadata) become fresh `arch` subjects
   keyed by FQN — never a guess at existing identity. LikeC4 is a
   renderer/model adapter: the DSL never becomes canonical storage.

## Repository layout

```text
crates/
├── vistalith-domain         # SubjectRef, VEvent, patch types, authority classes
├── vistalith-graph          # SWG, event projection, patches, behaviors, petgraph algorithms, context view
├── vistalith-agent-runtime  # conversation engine, frames, agents, provider contracts, MCP client, unified tools
├── vistalith-sddk-bridge    # governed SDDK promotion via the SDDK capability gateway (SPK-012)
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
docs/LIKEC4-SPIKE.md   # SPK-008 LikeC4 round-trip report
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
`POST /threads/{id}/messages/stream` (the same turn over Server-Sent
Events: `delta` frames while the model streams, a terminal `done` frame
with the durable coordinates),
`POST /threads/{id}/fork` (SPEC-011: copy items up to a turn with
`forked_of` bindings, link the fork back with `forked_from`),
`POST|GET /intents`, `GET /intents/{id}`, `POST /intents/{id}/promote`,
`POST /intents/{id}/discard` (SPEC-006 lifecycle), `GET /views/c4`,
`GET /tools` (unified catalog: native + MCP tools with permission decisions
and grant state), `POST /tools/{id}/grant` / `POST /tools/{id}/revoke`
(scoped temporary grants — write-class tools run only while a grant has
remaining calls), and `GET|POST /mcp/servers`,
`DELETE /mcp/servers/{name}` (SPEC-009: connect/disconnect MCP servers over
stdio or Streamable HTTP; discovered tools join the unified catalog with
consequences classified from MCP annotations — silent servers get the
conservative `write`),
`POST /views/context` (SPEC-005: bounded, explainable graph slice — roots,
relation allowlist, depth, authority filters and token budget, with an
inclusion/exclusion reason for every subject),
`GET /algorithms/impact/{namespace}/{kind}/{id}`,
`GET /algorithms/path?from=..&to=..`, `GET /algorithms/cycles` (ADR-007:
petgraph over an extracted snapshot; `?kinds=` restricts edge kinds),
`POST|GET /agents` (role, instructions, model profile, tools, budgets —
`AGENTS-DELEGATION.md`), `POST|GET /frames`, `GET /frames/{id}`,
`POST /frames/{id}/turns`, `POST /frames/{id}/close` (slice 8: bounded
execution — the frame owns a thread, its `permitted_tools` restrict the
unified catalog, and its turn/token budgets auto-close it: `completed`,
`aborted`, `turns-exhausted`, `budget-exhausted`),
`GET /sddk/receipts` (slice 9: receipts from the SDDK ledger when the
bridge is configured),
`GET /mcp/servers/{name}/health`, `POST /mcp/servers/{name}/refresh`
(re-discovery), `POST /mcp/servers/{name}/disable|enable` (slice 12:
disabled servers keep their registration while their tools leave the
unified catalog; a call that finds its transport dead reconnects once and
retries — the child process re-spawns), `POST /sddk/sync` (slice 10 / M6: project SDDK
ledger cycles into the SWG as derived `sddk:workflow:<id>` subjects,
idempotent by deterministic event ids), and
`GET /why/{namespace}/{kind}/{id}?depth=` (slice 10 / M9: the why-path —
incoming support links with the evidence backbone —
`provides_evidence_for` / `verifies` — highlighted),
`GET /lens/decisions` (slice 13: the decision lens — per decision, the
question, selected option, rejected alternatives, motivating requirement,
supporting evidence, contradictions and revisits, read from the typed
relations),
`POST /sddk/pull-up` (slice 14 / M10: evaluate a Vistalith innovation
against the SDDK focus test — gui/llm-free, semantic relevance, no
duplicated authority, deterministic — classify it deterministically
(VISTALITH_ONLY → SDDK_PROPOSAL) and, for proposals, submit it as governed
evidence through the SDDK capability gateway),
`POST /uat/checks` + `GET /lens/uat` (slice 15: durable UAT checks per
scenario with the latest verdict),
`POST /agents/{id}/run` (slice 18: run a goal on a defined agent — the
frame is created with the agent's instructions, tools and budgets, and the
run records structured outputs with contributes_to/executed_by
traceability),
`GET /algorithms/impact/{ns}/{kind}/{id}?full=true` (slice 16: the full
impact analysis — direct and transitive dependents, affected tests, stale
evidence, decisions possibly invalidated, and explicit unknown impact;
advisory only),
`POST|GET /canvas/subjects` and
`POST /canvas/subjects/{ns}/{kind}/{id}/promote` (slice 17: the thinking
canvas — note/question/hypothesis/option primitives as advisory subjects,
attached by mention, formalizing into VisualIntent drafts on demand),
`GET /views/c4/likec4` (slice 19 / SPK-008: the C4 projection as LikeC4
DSL, `text/plain` — every element carries its `SubjectRef` in
`metadata { vistalith ... }`) and `POST /views/c4/likec4` (import that DSL
back as durable events; identity-preserving no-op for untouched exports,
fresh FQN-keyed `arch` subjects for foreign models),
`GET /views/c4/diff?from=A[&to=B]` (architecture revision diff: added/
removed/changed elements and relationships on stable identities).
`POST /intents/{id}/promote` takes `approve`
(SPK-012: with the bridge enabled via `--sddk-ledger/--sddk-workflow/
--sddk-project`, promotions on SDDK-owned subjects submit a governed
proposal through the SDDK capability gateway — low risk executes and
receipts; high risk requires `approve: true`; undeclared capabilities are
denied by default).

Live event appends (`POST /events`) dispatch the built-in reactive
behaviors (SPEC-003): `impact-advisory` (a change to X advises every `X
depends_on` dependent), `contradiction-advisory`, `stale-evidence-advisory`
and `missing-evidence-advisory`. Advisories are durable advisory-class
subjects traced to their trigger via `causation_id`; replay never re-runs
behaviors, so replay stays byte-deterministic (milestone M4).

The web client also has a **Decisions** lens rendering that chain per
decision (M9: question → selected → rejected → evidence) and a
**Thinking** lens: sketch primitives, attach them to semantic subjects and
formalize them into VisualIntent drafts (progressive formalization).
The first pull-up candidate — the deterministic replay digest — is
submitted and reviewed in
[`docs/PULL-UP-REPLAY-DIGEST.md`](docs/PULL-UP-REPLAY-DIGEST.md).
The web chat streams assistant turns live (deltas render as they arrive).
The web client has three lenses over the same identities: **Graph**
(subjects/edges, with a time-travel selector and structural diff when
viewing a past revision), **C4** (projected view — with a LikeC4
round-trip section: export the DSL, edit it, import it back, and an
architecture diff between revisions) and **Chat** (threads,
with a per-thread fork action; copied items are marked `⎇ forked`, and the
tools panel lists the unified catalog where ask-class tools can be granted
or revoked).
Selecting a subject in any lens propagates the same `SubjectRef`.

MCP: connect a tool server at runtime —
`POST /mcp/servers {"name":"echo","command":"./target/debug/mcp-echo"}`
(stdio) or `{"name":"docs","url":"http://localhost:8100/mcp"}` (Streamable
HTTP). The `mcp-echo` fixture binary ships in the workspace for demos and
tests. `--provider fake --fake-tool TOOL_ID --fake-args '{...}'` scripts a
deterministic tool round for offline demos.

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
