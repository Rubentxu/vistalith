# SDDK as Vistalith Core

## Direct dependencies

Vistalith may directly use:
- `sddk-domain`;
- `sddk-engine`;
- `sddk-storage`;
- `sddk-gateway`;
- `sddk-vault`;
- `sddk-pack-uat` when required.

Avoid depending on `sddk-cli` for core behavior.

## Responsibilities retained by SDDK

- planning and WorkItem truth;
- workflow/run state;
- legal next actions and Decision Plane;
- SDDK policy/gateway;
- evidence and receipts;
- durable SDDK knowledge;
- Decision Memory;
- SDDK UAT semantics;
- deterministic lifecycle.

## Responsibilities added by Vistalith

- conversations;
- providers/models;
- Rig;
- MCP;
- agent interaction runtime;
- visual workspace;
- semantic world graph across product domains;
- LLM usage/tracing;
- Vistalith client protocol;
- rendering/lenses.

## Rule

Vistalith never reimplements a capability merely to avoid depending on SDDK.
If the capability belongs to SDDK, call SDDK directly.
