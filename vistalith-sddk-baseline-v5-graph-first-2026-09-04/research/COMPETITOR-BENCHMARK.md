# Competitor Benchmark — OpenCode, Claude Code, Codex

This is a product-capability benchmark, not an instruction to copy their internal
architecture.

## Capabilities Vistalith must match or exceed

### Interaction
- terminal/TUI;
- desktop;
- IDE/editor integration;
- persistent sessions;
- resume/fork/compact;
- streaming typed items;
- background/cancel;
- file/image attachments;
- semantic commands/skills.

### LLM control
- multiple providers where possible;
- local models;
- model catalog;
- reasoning/effort controls;
- model profiles;
- provider health;
- fallback;
- per-agent model choices;
- credentials;
- budgets and usage.

### Agents
- primary agent;
- subagents;
- isolated context;
- role/tool/permission restrictions;
- parallel work;
- visible delegation;
- structured contributions.

### Tools / MCP
- native tools;
- MCP stdio + remote;
- OAuth/auth;
- dynamic discovery;
- per-tool permission;
- health;
- output budgets;
- hooks/skills/plugins.

### Security
- allow/ask/deny;
- consequence-aware approvals;
- filesystem/network restrictions;
- no renderer secret access;
- trace every effect.

### Observability
- tokens/cost;
- provider/model;
- retries/reroutes;
- tool timings;
- approval waits;
- subagent spans;
- context composition.

## Where Vistalith differentiates

Modern coding agents are still primarily conversation/file oriented.

Vistalith adds:
- graph-native software model;
- `@SubjectRef`, not just `@file`;
- visual thinking;
- C4 as a live semantic lens;
- graph-reactive context and insights;
- Visual Intent;
- impact preview;
- semantic fork-and-diff;
- direct SDDK governed work;
- evidence/decision traceability;
- graph/time-travel exploration.

## Design lesson

Do not build "better chat" as the strategic differentiator. Chat is table stakes.
The differentiator is the semantic world behind it.
