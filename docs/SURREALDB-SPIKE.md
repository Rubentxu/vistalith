# SPK-003 — SurrealDB embedded storage spike (slice 5)

**Verdict: the decision gate is closed — SurrealDB is not adoptable today.**
Not because of performance (the engine measured well) but because it cannot be
built with any stable Rust toolchain this project can use. Storage remains
**Candidate B**: the durable JSON event log + in-memory projection that
already powers `vistalithd`. The domain never became SurrealQL-shaped — this
spike is an isolated, self-contained crate that can be deleted (or revived)
without touching any other crate.

Baseline references: `technology/GRAPH-STORAGE-DECISION.md` (the gate),
`graph/STORAGE-STRATEGY.md` (spike questions), `spikes/SPIKES.md` (SPK-003),
ADR-006 ("evaluate the stable 3.2.x line").

## What was built

`crates/vistalith-spike-surrealdb` — a standalone crate (own `rust-toolchain.toml`,
own `Cargo.lock`, excluded from the main cargo workspace) with:

- **Storage projection at the domain boundary**: `subject-defined` events
  upsert a `subject` record whose Surreal record key *is* the canonical
  `ns:kind:id` identity; `relation-declared` events create a native graph
  edge (`RELATE`) with a deterministic key `from|kind|to`, so replay is
  idempotent and byte-stable. Domain vocabulary in, storage rows out — no
  SurrealQL leaks anywhere else.
- **Determinism harness**: a canonical fact form (sorted subjects + sorted
  relations) built from both the in-memory `SemanticWorldGraph` and the
  SurrealDB rows; SHA-256 digests must match across (a) two fresh replays,
  (b) replay vs in-memory projection.
- **Gate runner** (`surrealdb-spike` binary): startup, idempotent schema
  re-application, bulk rebuild (batched multi-statement queries), in-memory
  replay comparison, 3-hop native traversal latency vs an in-memory walk,
  namespace isolation, durable reopen after close, binary/DB footprint.
- **Integration tests** (run on every spike `cargo test`): schema idempotency,
  determinism + domain-boundary digest equality, replay idempotency,
  traversal targets, namespace isolation, non-storage-event handling, and
  (behind `file-engine`) durable reopen.

## Why it is gated out: the toolchain wall

The baseline asks for the **stable 3.2.x line**. Measured reality
(reproduction: remove the spike crate from `exclude`, point the workspace dep
at `surrealdb = "=3.2.4"`, `cargo check`):

| Attempt | Result |
|---|---|
| `surrealdb 3.2.4` (baseline line) on rustc 1.91.0 | **fails to compile** — exact-pinned `diskann 0.54.0` errors with E0311 (`impl not general enough`, rust-lang/rust#100013) in `graph/index.rs` |
| `surrealdb 3.1.6` (newest stable) on rustc 1.91.0 / 1.90.0 / 1.89.0 | **fails identically** — `diskann 0.53.0`, same E0311 class |
| rustc 1.88.0 | fails earlier — `diskann-wide 0.53` needs a newer compiler |
| nightly (1.99.0-nightly, 2026-08-14) | **compiles and passes everything** |

`diskann` (vector search) is an unconditional, target-gated dependency of
`surrealdb-core` on all 64-bit non-WASM targets — there is no feature that
excludes it. Adopting SurrealDB today would therefore force the whole project
onto a nightly compiler, or onto waiting for an upstream fix we do not
control. That fails the spirit of the gate's "no unacceptable lock-in at the
domain boundary" before a single performance number matters, and it is why
the spike lives outside the main workspace with its own toolchain pin: a
spike must never gate product builds.

## Measured results (the engine itself, on nightly)

`surrealdb =3.1.6`, embedded SurrealKV file engine, release build, Linux
x86_64, 50 000 synthetic subjects + 66 666 `depends_on` relations
(116 666 statements, ring + chords so multi-hop traversals return data):

| Gate criterion | Measurement | Result |
|---|---|---|
| Deterministic migrations | schema re-applied in 1.0 ms, `OVERWRITE` defines, identical outcome | **PASS** |
| Deterministic rebuild | two fresh replays: same digest; SurrealDB rows == in-memory SWG projection (SHA-256 of canonical facts) | **PASS** |
| Rebuild throughput | 6 224 stmt/s batched (18.7 s) vs 787 ms full in-memory replay | measured |
| Traversal latency (interactive lens bar) | native 3-hop: p50 241 µs / p95 285 µs over 200 runs | **PASS** (p95 < 50 ms) |
| Traversal parity | targets reached at 3 hops: SurrealDB 2 == in-memory 2 (deduplicated sets) | **PASS** |
| Local durability | reopen after close: 741 ms (incl. async shutdown wait), counts + digest unchanged | **PASS** |
| Footprint | binary 56.6 MiB, DB 78.5 MiB at ~117k facts; startup to first query 396 ms | **PASS** (< 2 s, < 150 MiB) |
| Project-per-database isolation | second namespace sees zero rows; `use_ns` scoping is trivial | **PASS** |

1M-scale run (750 000 subjects + 999 998 relations = 1 749 998 statements,
same engine, same machine): rebuild sustains ~6 090 stmt/s (287 s), the
in-memory SWG replay of the same log took 14.1 s, native 3-hop traversal
stays flat at p50 304 µs / p95 348 µs (vs 241/285 µs at 117k facts), durable
reopen after close 9.1 s, DB size 1.18 GiB, binary 56.6 MiB — and every
determinism/digest check still passes, including `digest == in-memory` at
full scale. `/tmp` reports reproducible via `--nodes 750000`.

Notable honest observations:

- The **in-memory** 3-hop walk measured *slower* than SurrealDB's native
  traversal (p50 ≈ 7.5 ms) — but only because the SWG currently keeps all
  relations in one ordered map without an adjacency index, so each hop scans
  every edge. That is a projection data-structure gap, not a storage verdict;
  an adjacency map would put the in-memory walk back in the microseconds.
- Embedded close is asynchronous: reopening immediately after dropping the
  client races the release of the SurrealKV LOCK (the spike retries with
  backoff). A future adapter would need explicit lifecycle handling.
- Aggregates (`count()` with `GROUP ALL`) return objects, and selecting from
  a table that does not exist in the current namespace is an error rather
  than an empty result — 3.x strictness worth knowing before any integration.

## Decision

Per `GRAPH-STORAGE-DECISION.md`: **the fallback stands.** Retain graph-first
domain semantics; storage stays as the durable event log + strict in-memory
projection (deterministic replay, SHA-256 digest verification, JSON fixture
portability — all already enforced by `vistalith-graph` tests). Revisit this
spike when `diskann`/surrealdb ship a release that compiles on stable rustc;
the spike crate is designed to re-run the entire gate unchanged at that point:

```bash
cd crates/vistalith-spike-surrealdb
cargo test --features file-engine
cargo run --release --features file-engine -- --engine surrealkv --nodes 50000 \
  --json-out /tmp/spike-report.json
```
