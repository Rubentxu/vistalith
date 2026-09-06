# SPK-009 — Excalidraw semantic bindings

**Scope (baseline `spikes/SPIKES.md`):** persist semantic bindings
independently from shape IDs. Normative constraints: ADR-014 ("use behind a
`CanvasPort`. Semantic identities remain outside canvas-native IDs") and
`visual/VISUAL-THINKING.md` ("Never store Excalidraw IDs as canonical
semantic identity").

## The problem

Excalidraw shape ids are renderer-local: copy-paste renumbers them, a
foreign tool round-trip drops them, and nothing in a `.excalidraw` file is
guaranteed stable. A binding stored as `(scene, shape_id) → subject` breaks
the first time the renderer sneezes.

## What was built (slice 20)

- **Bindings live in the graph, not the canvas.** Each binding is a durable
  advisory subject (`vistalith:sketch-element:<uuid>`, new `canvas-bound`
  event) linked to its semantic subject with a `visualizes` relation. It
  carries: scene name, the shape id **at binding time** (provenance only),
  a content fingerprint, the binding `via`, and optional geometry
  (position/size bookkeeping for faithful re-rendering).
- **Content fingerprint** — SHA-256 over `shape_type + normalized text`
  (whitespace-collapsed, case-folded). Moving a shape or renumbering it
  never changes the fingerprint; editing its text does (a semantically
  different shape).
- **Identity travels inside the scene file** — exported elements carry
  `customData { vistalith: "ns:kind:id" }`, the same pattern as the LikeC4
  metadata round-trip. Excalidraw's `customData` survives export/import
  with the element, so identity follows the shape even through foreign
  tools that preserve unknown fields.
- **Import is content-keyed.** Bindings are `(scene, fingerprint,
  subject)` facts:
  - `customData.vistalith` present and valid → bind to that identity
    (round-trip). Invalid metadata is a hard error; a ref to a subject the
    graph does not know is reported (`unknown_subjects`), never invented.
  - No customData: a unique fingerprint match in the scene means the
    content is already bound — an idempotent skip, **even though the shape
    id changed**. Several subjects sharing the same text is ambiguity:
    the shape stays unbound (never guess).
  - `create_missing=true` (explicit opt-in): unbound TEXT shapes become
    canvas `note` primitives (advisory subjects) and bind to them —
    progressive formalization step 1, nothing executes.
- **Export** — canvas primitives (note/question/hypothesis/option, slice
  17) become text elements with `customData.vistalith`; geometry comes
  from the latest stored binding when present, else a deterministic grid.
  Canonically ordered; the same graph yields byte-stable scenes.
- **Surface** — `GET|POST /canvas/excalidraw` (`?scene=&create_missing=&
  actor=`), `GET /canvas/bindings?scene=`; client `canvasScene()`,
  `importCanvasScene()`, `canvasBindings()`; an Excalidraw section in the
  web Thinking lens (export → paste/edit → import → report).

## Why this satisfies the spike

- A scene can be exported, its ids renumbered by any tool, and re-imported:
  bindings resolve through `customData` or content, and the graph is
  unchanged (no-op) — verified at unit, HTTP and live-smoke level.
- Strip `customData` entirely: content still resolves to the same subject.
- Two shapes with the same text are ambiguous by design; import refuses to
  choose and reports them unbound.
- The renderer (Excalidraw or any future `CanvasPort`) holds no authority:
  deleting a scene loses nothing semantic; the graph can always re-emit a
  scene from the bindings.

## Evidence

- 10 unit tests in `crates/vistalith-graph/src/excalidraw.rs`: fingerprint
  normalization, custom-data binding, round-trip no-op with renumbered ids,
  customData-loss re-import, ambiguity refusal, `create_missing`
  primitives + idempotent re-import, unknown identities, broken scenes,
  export with binding geometry and deterministic grid, per-scene filtering.
- Projection: the `canvas-bound` arm is strict (subject must exist,
  binding subject fresh) and deterministic — replay never re-runs imports
  because imports are plain events.
- 3 HTTP tests in `crates/vistalith-server/tests/api.rs` covering the
  full loop through `vistalithd` (bind → read model → export → re-import
  no-op with unchanged revision), `create_missing` visibility in the
  canvas lens, and broken-scene rejection.
- 2 web component tests; live smoke (see README slice 20): export →
  renumber ids + strip customData → re-import leaves the revision
  untouched.

## Verdict

**Bindings belong to the SWG; shape ids never did.** The combination of
durable `visualizes` bindings, content fingerprints and `customData`
identity makes the Excalidraw canvas a disposable view — the same
disposability the LikeC4 DSL export has. Accepted limitation: the
fingerprint is text-based, so geometry-only shapes (arrows, boxes without
labels) bind only via `customData`; a future `CanvasPort` can extend the
fingerprint with stable shape semantics if a measured need appears.
