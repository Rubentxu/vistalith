# SPK-008 — LikeC4 round-trip

**Scope (baseline `spikes/SPIKES.md`):** round-trip C4 node ↔ `SubjectRef`
and architecture revision diff. Related normative doc:
`visual/C4-ARCHITECTURE.md` ("LikeC4 remains the preferred first-class C4
renderer/model adapter… Architecture subjects are SWG nodes; C4 is one
projection").

## What was built (slice 19)

- **Export** — `vistalith_graph::likec4::likec4_source(graph)` renders the
  C4 projection (`crates/vistalith-graph/src/c4.rs`) as LikeC4 DSL:
  `specification` (element kinds in use, one `relationship` per relation
  kind, `tag deprecated` when needed), a flat `model` with one element per
  C4 node and `source -[kind]-> target` relationships, and an `overview`
  view. Deterministic: the same revision always yields byte-identical
  output.
- **Identity travels in metadata.** Every exported element carries
  `metadata { vistalith 'ns:kind:id' }`. Element ids in the DSL are
  cosmetic (sanitized identifiers; collisions deduplicated with `-2`,
  `-3`, …); identity never depends on them.
- **Import** — `parse_likec4(source)` accepts a pragmatic DSL subset:
  nested elements with lexical FQN scoping, typed/untyped relationships,
  `this`/`it`, `extend` (name/description/metadata/tags merge), `metadata`,
  tags, triple-quoted strings, comments; `views`, `deployment`, `global`
  and `import` statements are skipped. `import_likec4(store, model, actor)`
  appends durable events:
  - `metadata.vistalith` parses to a `SubjectRef` → that identity is
    reused (round-trip). Unparseable metadata is a hard error, never a
    silent new subject.
  - No metadata → a fresh `arch` subject keyed by the LikeC4 FQN.
  - Element kinds map: `system/container/component/interface/datastore/
    deploymentnode`; anything else is rejected (`UnsupportedElementKind`).
  - Existing subjects are updated only on a real property diff;
    re-importing an untouched export appends **nothing**.
  - `#deprecated` maps onto the first-class deprecation fact; other tags
    become a `likec4_tags` property. Untyped relationships land on the
    generic `uses` verb; typed ones map through `RelationKind::parse`
    (camelCase/hyphenated kinds are normalized to snake_case).
- **Architecture revision diff** — `c4_diff(from, to)` projects both
  revisions into C4 views and reports added/removed/changed elements and
  relationships on stable identities: a rename is a `name` property change
  on the same `SubjectRef`, never remove+add.

## API surface

- `GET /views/c4/likec4` — the current projection as `text/plain` DSL.
- `POST /views/c4/likec4?actor=...` — import a DSL body as durable events;
  returns the import report (defined/updated/unchanged/deprecated subjects,
  declared/skipped relations).
- `GET /views/c4/diff?from=A[&to=B]` — architecture revision diff.
- Client: `likec4Model()`, `importLikec4(source)`, `c4Diff(from, to)`;
  web UI: LikeC4 section in the C4 lens (export → edit → import → report,
  plus the diff viewer).

## Evidence

- 14 unit/integration tests in `crates/vistalith-graph/src/likec4.rs`:
  export determinism, full round-trip equality of the C4 views, re-import
  no-op (revision unchanged), digit-leading id sanitization with identity
  preserved via metadata, property-level updates on edited DSL, nested FQN
  scoping with `this` and typed arrows, `extend` merging, FQN-keyed foreign
  imports, unsupported-kind and invalid-metadata rejections, explicit
  parse errors (unknown ref, ambiguous suffix, unterminated string),
  comment/view tolerance, and `c4_diff` rename/add semantics.
- 4 HTTP tests in `crates/vistalith-server/tests/api.rs`: export content
  type + embedded identity, re-import is a no-op (revision unchanged),
  foreign model import creates FQN subjects, broken DSL → 422, diff
  endpoint output.
- Live smoke against `vistalithd` (see README status table, slice 19):
  export → import round-trip leaves the revision untouched; renaming a
  system and re-diffing shows exactly one changed element.
- **Real LikeC4 CLI validation** (`pnpm dlx likec4@1.53.0 validate --json
  --no-layout`): both the pristine export and the edited model validate
  clean (`valid: true`, 0 errors). A hand-edited model that uses a typed
  relationship without declaring it in `specification` is rejected by
  LikeC4 ("Could not resolve reference to RelationshipKind") — correct
  upstream behavior; Vistalith's import is deliberately more permissive
  and does not require a specification block.

## Verdict

**LikeC4 works as the C4 model adapter, with the SWG staying canonical.**
The DSL is a serialization format for the projection, never storage:
identity, authority and provenance live in the graph, and the metadata
round-trip keeps renderer/model files disposable. Limitations, accepted for
the spike: the parser covers the architecture-model surface (not dynamic
views, deployment topologies or view predicates beyond skipping them), and
export is flat (no nesting), which keeps the parent-child relationship
constraint of LikeC4 moot.
