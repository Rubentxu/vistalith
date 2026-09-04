# Dependency pins

Rule (`technology/STACK.md`): exact pins live in manifests/lockfiles; docs name
the tested stable line. This file records where each pin lives.

## SDDK core

Vistalith consumes SDDK Rust crates directly (ADR-001). The pinned revision is
kept in an intermediate, machine-local checkout:

```text
dev/sddk-framework/   <- git clone of software-development-decision-kernel,
                         detached at the pinned revision (gitignored)
```

- Current pin: **`v1.82.0`** (`d43b120b6e67d467033acd61f7f3c286559a97b7`)
- Pin record: `scripts/sddk-pin.env` (committed — this is the source of truth)
- Crates consumed: `sddk-domain`, `sddk-engine`, `sddk-storage`, `sddk-gateway`,
  `sddk-vault`, `sddk-testkit` (`architecture/SDDK-AS-CORE.md`; `sddk-cli` is
  not a core dependency). Path deps are declared once in the root
  `Cargo.toml [workspace.dependencies]`.
- Bootstrap/refresh: `scripts/bootstrap-dev.sh` (checkout at pin) and
  `scripts/bootstrap-dev.sh --pin <tag>` (move the pin).

### Pin policy (architecture/DEPENDENCY-MODEL.md)

- Never mix SDDK crate revisions: every consumed crate resolves to the same
  commit because they all live in the one pinned checkout.
- Only pin revisions that exist on the SDDK origin (tags preferred) so CI and
  other machines can reproduce the checkout.
- An SDDK upgrade is a first-class dependency upgrade: update pin, compile, run
  contract/graph projection tests, run master UAT, inspect semantic diff,
  accept or revert.
- No compatibility façade: compile errors are evidence of real coupling.

### sddk CLI binary

The `sddk` CLI binary built from the pinned checkout is fixed in `dev/bin/`
(`sddk-<version>-<target>` plus `.sha256`), alongside the release metadata
(attestation/SBOM) when produced by the SDDK release pipeline. Agents and gates
must use this pinned binary, never a `cargo install` of a moving revision.

## Rust crates (crates.io lines per technology/STACK.md)

| Crate | Line | Where pinned |
|---|---|---|
| SDDK crates | v1.82.0 (git) | `scripts/sddk-pin.env` + root `Cargo.toml` |
| `rig-core` | 0.42.0 (consumed by vistalith-agent-runtime, adapter only) | root `Cargo.toml` + `Cargo.lock` |
| `rmcp` | 3.2.x | root `Cargo.toml [workspace.dependencies]` |
| `petgraph` | 0.8.x | root `Cargo.toml [workspace.dependencies]` |
| Tokio/Axum/Serde/Tracing | current stable | root `Cargo.toml [workspace.dependencies]` |
| everything else | exact | `Cargo.lock` (committed) |

## TypeScript stack

Node ≥24 LTS, pnpm 12 (via `packageManager` in the root `package.json`),
TypeScript 7.0.2, React 19.2.8, Vite 8.2.2, TanStack Query 5, Zustand 5,
Vitest 5, Testing Library, Biome 2.5. Exact pins live in the package.json
files (`.npmrc` sets `save-exact=true`) and `pnpm-lock.yaml` (committed).
Playwright stays unpinned until the first e2e slice.

## Toolchain

Rust 1.91.0 via `rust-toolchain.toml` (matches SDDK's `rust-toolchain.toml`).
