# Technology Baseline

Dated baseline: 2026-09-04.

## Rust backend
- SDDK: pinned compatible revision/release.
- `rig-core`: 0.42.x baseline.
- `rmcp`: 3.2.x baseline.
- `petgraph`: 0.8.x baseline.
- SurrealDB: evaluate stable 3.2.x embedded line.
- Tokio/Axum/Serde/Tracing/OpenTelemetry: pin current compatible stable releases
  when repository is bootstrapped; keep versions in Cargo workspace dependency table.

## TypeScript
Use the previously validated modern stack:
- Node 24 LTS line;
- pnpm 12;
- TypeScript 7 stable;
- React 19.2;
- Vite 8;
- Tauri 2;
- TanStack Query/Router where needed;
- Zustand for compact client interaction state;
- Vitest;
- Playwright;
- Biome.

## Visual
- LikeC4;
- Excalidraw;
- React Flow;
- ELK;
- Monaco/Xterm later.

## Version rule
Exact pins belong in manifests/lockfiles. Documentation names the tested stable
line and is updated during dependency upgrades.
