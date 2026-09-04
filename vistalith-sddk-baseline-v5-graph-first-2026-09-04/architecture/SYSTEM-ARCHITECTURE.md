# System Architecture

```text
┌──────────────────────────────────────────────────────────────┐
│                    Vistalith Clients                         │
│             TypeScript / React / Tauri / TUI                 │
│                                                              │
│ Chat  Thinking  C4  Workflow  Agents  Decisions  Impact      │
│ Models MCP Tools Trace Usage Code Terminal UAT Evidence      │
└─────────────────────────────┬────────────────────────────────┘
                              │
                     Vistalith protocol
                              │
┌─────────────────────────────▼────────────────────────────────┐
│                         vistalithd                            │
│                             Rust                             │
│                                                              │
│ Conversation │ Agent Runtime │ Context │ LLM │ MCP │ Tools    │
│                                                              │
│                  Reactive Semantic Graph                     │
│    event log → patches → projections → patterns → views      │
│                                                              │
│           Visual Intent / Semantic Proposal Engine            │
│                                                              │
│      Direct in-process use of SDDK Rust crates               │
│                              │                               │
│   ┌──────────────────────────▼────────────────────────────┐   │
│   │                       SDDK                            │   │
│   │ Planning │ Workflow │ Decision │ Evidence │ Gateway  │   │
│   │ Vault │ UAT │ deterministic lifecycle semantics      │   │
│   └───────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────┘
```

## Architectural asymmetry

Vistalith depends on SDDK. SDDK does not depend on Vistalith.

## Shared process, explicit ownership

Direct dependency does not mean blurred domain ownership. SDDK-owned facts remain
owned by SDDK. Vistalith may project them into its graph and relate them to
chat/LLM/visual subjects.

## No internal SDDK protocol

The only rich-client protocol required is `Vistalith Protocol`, between
`vistalithd` and UI clients.
