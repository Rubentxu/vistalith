# Tracing, Usage and Cost

Trace hierarchy:

```text
Thread
 └─ Turn
    ├─ ContextCompile
    ├─ ModelCall
    │  ├─ stream
    │  └─ tool request
    ├─ ToolExecution
    ├─ SubagentRun
    ├─ SDDK operation
    └─ Synthesis
```

Capture:
- model/provider/profile;
- tokens/cached/reasoning where available;
- estimated/provider cost;
- time-to-first-token;
- total latency;
- retries/reroutes;
- tool/approval wait;
- context size;
- graph revision;
- SDDK WorkItem/run IDs;
- correlation/causation IDs.

Use OpenTelemetry GenAI conventions where useful plus Vistalith/SDDK semantic
attributes.
