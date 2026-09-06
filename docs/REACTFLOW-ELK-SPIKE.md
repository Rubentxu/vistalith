# SPK-010 — React Flow / ELK at scale

**Scope (baseline `spikes/SPIKES.md`):** 1k/10k nodes, off-main-thread
layout, live status updates. Normative constraints: ADR-015 ("React Flow +
ELK for workflow/agent lens. Projection only; SDDK remains workflow
authority") and `visual/WORKFLOW-AGENTS.md` ("No second workflow model" —
the graph visualizes SDDK workflow semantics, never an independent
execution DAG authority).

## What was built (slice 21)

- **Flow lens** in the web client: a new `Flow` tab renders the workflow
  plane of the SWG — `workflow`, `workflow-node`, `workflow-run`, `agent`
  and `frame` subjects with their `contains` / `depends_on` /
  `executed_by` / `contributes_to` relations — over React Flow
  (`@xyflow/react` 12) with ELK layered layout.
- **Off-main-thread layout (the spike's core requirement).** The ELK
  runner (`apps/web/src/flow/layout.ts`) is a pure function. The lens runs
  it inside a Vite module Web Worker (`layout.worker.ts`); the main thread
  only merges the returned positions. A same-thread fallback covers
  workerless environments (tests); a result token discards stale layouts
  when the structure changed again while computing.
- **Live status updates without re-layout.** The layout trigger is a
  structural signature — the node/edge id sets only, deliberately not the
  graph revision. The lens reuses the shared 2 s-polled graph query, so a
  `workflow-run` flipping `running → completed` re-renders the badge in
  place (positions untouched, zero layout work) while adding/removing a
  node or edge re-layouts exactly once.
- **Scale mechanisms:** React Flow `onlyRenderVisibleElements`
  (viewport virtualization), fixed node size (176×44), disabled dragging,
  mini-map + controls for navigation at min zoom 0.02.
- **Bench tooling:** `scripts/gen-flow-bench.mjs` generates a
  deterministic raw-VEvent fixture (same shape as the slice-1 fixture) —
  `--nodes 10000` yields 13,900 subjects / 24,600 relations / 38,500
  events that `vistalithd --fixture` loads directly. At 1k nodes
  (`--nodes 1000`): 1,390 subjects, 2,460 relations, `GET /graph` ≈ 1 MB.
  `scripts/measure-elk.mjs` times the ELK layout over the same shapes.

## Measurements (this machine, Node 25, ELK 0.9.3 bundled, layered/DOWN)

| Graph | ORTHOGONAL | POLYLINE |
|---|---|---|
| 139 nodes / 231 edges | ~0.72 s | — |
| 1,390 nodes / 2,310 edges | 7.8 s median | 7.5 s median |
| 13,900 nodes / 23,100 edges | 32.5 s median | 32.9 s median |

The edge-routing choice is irrelevant; the layered algorithm dominates the
cost. These numbers are the spike's decisive evidence: **a 10k layout is a
~30 s computation and must never touch the main thread** — even the 1k
case would freeze the UI for several seconds.

Browser-side rendering is virtualized (React Flow renders only visible
nodes) and layout runs in the worker, so the main thread stays reactive
during the multi-second computation; the panel surfaces honest state
(`layouting… → layout N ms`, `worker layout`) instead of pretending
instant layout. Production follow-ups implied by the numbers: for 10k+
graphs, either layout progressively (per expanded `workflow` subgraph,
which the `contains` hierarchy makes natural) or use a cheaper layout for
the initial view and upgrade to ELK on demand.

## Live status semantics (test-enforced)

- Same nodes/edges + new statuses → **no** new layout call, badge text
  updates in place (positions stable).
- Adding a node → exactly one new layout call.
- Verified in `apps/web/tests/flow.test.tsx` with the layout module
  spied; layout determinism and coverage asserted in
  `flow-layout.test.ts` against real ELK.

## Verdict

**React Flow + ELK is the right workflow/agent lens adapter, with one
operating rule learned at scale: layout is an asynchronous background
computation, not a render step.** The layered algorithm gives readable
hierarchies at studio scale (hundreds of nodes); at warehouse scale (10k)
it is a background job — the worker architecture this slice ships is a
requirement, not an optimization, and per-workflow progressive layout is
the natural next step. SDDK stays the only workflow authority: the lens
projects `workflow-run` statuses and agent activity from graph facts and
owns no execution state.

Adopted limitations: edge labels are the relation kinds (no orthogonal
bundling); node visuals are kind-colored boxes (custom node cards with
evidence/cost overlays belong to V9 productization); build chunk warning
from the ELK bundle (~1 MB) is acceptable for the spike and can be
lazy-loaded per lens later.
