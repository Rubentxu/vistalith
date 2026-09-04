# Rig Strategy

## Baseline
Evaluate `rig-core 0.42.x` as the principal provider/completion abstraction.

Useful capabilities:
- common provider abstractions;
- completion/streaming;
- structured output;
- tool schemas;
- embeddings where useful;
- access to provider raw responses;
- GenAI-oriented integration ecosystem.

## Rule

Rig types do not become Vistalith domain types.

Own Vistalith contracts:
- ModelRequest;
- ModelEvent;
- ModelUsage;
- ModelDescriptor;
- ProviderHealth;
- ToolRequest.

Rig is an adapter underneath.

## rig-agent

Do not make `rig-agent` the authoritative Vistalith loop at baseline.

Run a spike. Adopt only mechanics that reduce complexity without taking
ownership of:
- conversations;
- graph/event state;
- permissions;
- SDDK integration;
- trace semantics;
- agent roles/delegation.
