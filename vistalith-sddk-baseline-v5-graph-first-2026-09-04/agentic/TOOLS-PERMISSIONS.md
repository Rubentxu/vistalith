# Tools and Permissions

## Tool sources
- Vistalith native;
- SDDK direct capability;
- MCP;
- later plugin/WASI/process.

## ToolDescriptor
- ID;
- schema;
- consequence class;
- read/write/network/destructive hints;
- timeout;
- output budget;
- required capabilities;
- source;
- health.

## Permission outcomes
- deny;
- allow;
- ask.

Support scoped temporary grants.

## Composition

Vistalith permission may further restrict an operation.

It may never weaken SDDK policy for SDDK-governed effects.
