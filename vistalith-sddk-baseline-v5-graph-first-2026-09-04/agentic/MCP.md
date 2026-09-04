# MCP Runtime

## Baseline
Use Rust MCP SDK (`rmcp` 3.2.x line) directly unless the Rig adapter proves
strictly better for a specific seam.

## Server model
- local stdio/child process;
- Streamable HTTP;
- authenticated remote;
- required/optional;
- enabled/disabled;
- health;
- reconnect;
- server instructions;
- tool/resource/prompt discovery.

## Security
- credentials outside renderer;
- per-project/user/agent scope;
- per-tool permission;
- output/token budget;
- timeout;
- consequence classification.

## Unified catalog
MCP tools and native tools project into one ToolCatalog.

## SDDK
An MCP tool cannot bypass SDDK's gateway if the requested effect is an
SDDK-governed engineering operation.
