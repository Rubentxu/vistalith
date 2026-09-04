# Event-Sourced Graph

## Durable Vistalith event

```text
VEvent
├─ event_id
├─ type
├─ aggregate/subject refs
├─ correlation_id
├─ causation_id
├─ revision
├─ actor
├─ payload
├─ timestamp
└─ trace_id
```

## Flow

```text
command / external observation
        ↓
VEvent
        ↓
append durable log
        ↓
project
        ↓
Semantic World Graph revision
        ↓
patterns / reactive behaviors
        ↓
zero or more proposals/events
```

## Sources

Not every SDDK event must be duplicated as Vistalith authority. Vistalith may
record observation/projection events that link to SDDK IDs/revisions.

## Replay

Support:
- strict projection replay;
- rebuild materialized graph;
- replay selected Vistalith behavior families;
- no replay of external LLM/tool calls unless explicitly using recorded fixtures.

## Determinism classes

- deterministic projection behavior;
- deterministic rule behavior;
- recorded-external behavior;
- live non-deterministic behavior.

The class is explicit per behavior.
