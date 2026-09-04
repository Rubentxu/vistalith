# Conversation Runtime

## Model

```text
Project
 └─ InteractionSession
     └─ Thread
         ├─ bound SubjectRefs*
         ├─ Turn*
         │   └─ Item*
         └─ forks*
```

## Operations

- create;
- resume;
- search/list;
- rename/pin/archive;
- cancel/background;
- fork at turn;
- compact;
- export sanitized trace;
- bind/unbind semantic subjects;
- compare branches.

## Persistence

Conversation state is Vistalith-owned and event-sourced enough to reconstruct
thread chronology and typed items.

Chat transcript is not SDDK Decision Memory.
