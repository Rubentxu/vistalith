# Proposed Rust Crate Topology

```text
vistalith/
├── crates/
│   ├── vistalith-domain
│   ├── vistalith-events
│   ├── vistalith-graph
│   ├── vistalith-reactive
│   ├── vistalith-conversation
│   ├── vistalith-agent-runtime
│   ├── vistalith-llm
│   ├── vistalith-mcp
│   ├── vistalith-tools
│   ├── vistalith-context
│   ├── vistalith-visual-intent
│   ├── vistalith-storage
│   ├── vistalith-protocol
│   └── vistalith-server
├── apps/
│   ├── web
│   ├── desktop
│   └── terminal
└── packages/
    ├── client
    ├── ui
    └── lenses/*
```

## Start smaller

Do not necessarily create all crates in commit 1.

Recommended initial crates:
- `vistalith-domain`;
- `vistalith-graph`;
- `vistalith-agent-runtime`;
- `vistalith-server`.

Extract event/reactive/LLM/MCP crates only after boundaries are demonstrated by
real dependencies and tests.

## Dependency direction

```text
domain
 ↑
graph / conversation / visual-intent
 ↑
agent-runtime / context / tools
 ↑
server
```

SDDK crates are direct dependencies where semantically appropriate.
