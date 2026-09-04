# Vistalith

Agentic visual engineering workspace with a **Semantic World Graph** at its
core, built directly on top of the
[SDDK](https://github.com/Rubentxu/software-development-decision-kernel)
crates (ADR-001).

Baseline: `vistalith-sddk-baseline-v5-graph-first-2026-09-04/` — read
[`START-HERE.md`](vistalith-sddk-baseline-v5-graph-first-2026-09-04/START-HERE.md)
in that folder for the normative reading order.

## Repository layout

```text
crates/
├── vistalith-domain   # SubjectRef (ADR-011), VEvent (SPEC-002), patches, authority
├── vistalith-graph    # in-memory SWG, event projection, patches, deterministic replay
└── vistalith-server   # `vistalithd` — tiny axum server over the event log + SWG
packages/
└── client             # @vistalith/client — TS mirror of the protocol + typed HTTP client
apps/
└── web                # React/Vite graph lens: subjects/edges, selection by SubjectRef
dev/                   # pinned SDDK checkout + pinned sddk CLI binary (gitignored)
docs/DEPENDENCIES.md   # every dependency pin and the pin policy
vistalith-sddk-baseline-v5-graph-first-2026-09-04/  # planning baseline (docs)
```

## First slice status (roadmap/IMPLEMENT-NOW.md)

- [x] Rust workspace, Rust 1.91, edition 2024.
- [x] Direct SDDK path dependencies pinned at `v1.82.0`.
- [x] `SubjectRef` — stable, revision-aware identity; renderer IDs are never
      semantic IDs; identity string `namespace:kind:id`.
- [x] `VEvent` — durable event with correlation/causation, log-assigned
      sequence + revision; rejected patches are events too.
- [x] In-memory SWG (simple ordered maps; petgraph arrives with ADR-007).
- [x] Deterministic fixture replay: the same raw log always produces the same
      SHA-256 graph digest; rebuilds are verified against stored revisions.
- [x] Tiny `vistalithd` (axum): health, graph, subjects, events, patches.
- [x] `@vistalith/client`: typed TS mirror of `SubjectRef`/`VEvent`/patches +
      fetch client (pnpm workspace, exact pins, pnpm 12).
- [x] Web graph lens (`apps/web`): React 19 + Vite 8 + TanStack Query +
      Zustand; namespace-column SVG of subjects/edges; selection propagates
      `SubjectRef`s across list, graph and details lenses (never renderer
      IDs); adjacency highlight, authority-colored facts, advisory edges
      dashed; CORS enabled on `vistalithd` for the dev origin.
- [ ] SurrealDB spike (only after semantics/tests — gate pending).
- [ ] First conversation thread, one provider through Rig, one C4 projection.

## SDDK dependency pinning

The SDDK revision Vistalith compiles against is fixed in an intermediate
machine-local site (`dev/`), never on a moving working checkout:

```bash
scripts/bootstrap-dev.sh              # clone + checkout the pinned revision
scripts/bootstrap-dev.sh --pin v1.83.0  # move the pin (updates scripts/sddk-pin.env)
```

- Pin record (committed): `scripts/sddk-pin.env` → currently
  `v1.82.0` (`d43b120b6e67d467033acd61f7f3c286559a97b7`).
- Path dependencies are declared once in the root `Cargo.toml`
  (`[workspace.dependencies]`, `exclude = ["dev"]`).
- The `sddk` CLI binary built from the pinned checkout is fixed in
  `dev/bin/` with a `.sha256` and `.pin.json` manifest.
- Rules: never mix SDDK revisions; only pin refs that exist on the origin;
  upgrades follow `architecture/DEPENDENCY-MODEL.md` (compile → contract
  tests → master UAT → semantic diff → accept or revert).

## Running

Rust core (24 tests):

```bash
cargo test
cargo run -p vistalith-server --bin vistalithd \
  --fixture crates/vistalith-graph/tests/fixtures/sample-world.json --port 7420
```

HTTP API: `GET /health`, `GET /graph`, `GET /subjects`,
`GET /subjects/{namespace}/{kind}/{id}`, `GET|POST /events`,
`POST /patches` (applied → `200`, rejected → `409`; rejections are durable
events, see `GET /events`).

TypeScript workspace (24 tests; Node ≥24, pnpm via `packageManager`):

```bash
pnpm install
pnpm build       # client (tsc) + web (vite)
pnpm test        # vitest in both packages
pnpm lint        # biome
pnpm dev:web     # http://localhost:5173, talks to vistalithd on :7420
```

Set `VITE_VISTALITHD_URL` to point the web client at another `vistalithd`.

## Invariants enforced in code

- Graph patches carry the base revision; a stale base is rejected
  (SPEC-004 optimistic concurrency).
- Patches that would authoritatively mutate an SDDK-namespace subject are
  rejected with `must-be-governed-by-sddk`: convert them into governed SDDK
  semantic proposals (SPEC-001 invariant 4 / SPEC-004). Vistalith holds SDDK
  truth as *derived observations* with provenance, never as its own authority.
- Graph digest is canonical (ordered containers), so replay determinism is
  checkable across processes.
