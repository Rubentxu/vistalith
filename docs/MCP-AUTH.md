# SPK-007 — MCP remote auth (spike closed)

**Scope:** the last open item of `spikes/SPIKES.md` #SPK-007 ("stdio +
Streamable HTTP + auth + tools changed + reconnect"). stdio, Streamable
HTTP, reconnection and tools-changed shipped in slices 6 and 12; slice 22
adds **auth** for remote Streamable HTTP servers, closing the spike.

## Design (slice 22)

- **`McpAuth` on the server config** — two shapes, tagged `type`:
  - `bearer` — `Authorization: Bearer <token>`;
  - `header` — any static header (e.g. `x-api-key`), name + value.
  The secret may be inline or **environment-referenced** (`token_env` /
  `value_env`), resolved at connect/reconnect time so config files never
  need to hold credentials.
- **Every request authenticates.** The transport is built from a
  preconfigured `reqwest` client carrying the credential as a default
  header, so `initialize`, tool calls and the SSE stream all carry it.
  Reconnect re-opens the session from the same config — env-referenced
  secrets re-resolve (a rotated secret is picked up on the next reconnect).
- **Secrets never leave the process** (SPEC-008 discipline):
  - status and health expose only a redacted kind — `bearer` or
    `header:<name>`;
  - error messages name a missing env variable, never its value;
  - the tool-call path and logs carry no credential material.
- **Transport discipline:** `auth` on a stdio server is invalid config —
  rejected in validation and again at the HTTP API boundary (422).

## Evidence

- E2E suite `crates/vistalith-agent-runtime/tests/mcp_http_auth.rs` against
  a **real rmcp Streamable HTTP server** behind an auth gate: bearer
  connect + tool discovery, missing/wrong credentials fail to connect,
  custom header auth, reconnect re-authenticates, env-referenced token
  resolves and a missing env variable fails with the variable named, and
  the serialized status contains no secret.
- API-boundary tests (`crates/vistalith-server/tests/tools_api.rs`): the
  server listing never reports credential-shaped fields; stdio+auth is
  rejected with a 422.
- Live smoke through `vistalithd`: config with `auth` on stdio → 422 with
  the explanation; stdio echo server registers connected with 2 tools;
  status/health show no `auth` field when none is configured (the field
  appears only redacted when set).

## Verdict

**SPK-007 is closed:** stdio + Streamable HTTP + auth + tools-changed +
reconnect all demonstrated against real servers. OAuth-style dynamic
flows (authorization-code with a browser redirect) remain out of scope
for the single-user local product (B10: remote/collaboration waits for a
measured need); static credentials with env indirection cover the
realistic remote-tool deployments for now.
