# Start Here

## Normative reading order

1. `BASELINE.md`
2. `VISION.md`
3. `PRINCIPLES.md`
4. `architecture/SYSTEM-ARCHITECTURE.md`
5. `architecture/SDDK-AS-CORE.md`
6. `graph/SEMANTIC-WORLD-GRAPH.md`
7. `graph/ACTIVEGRAPH-DESIGN-TRANSFER.md`
8. `agentic/AGENTIC-INTERACTION-PLANE.md`
9. `visual/VISUAL-THINKING.md`
10. `visual/VISUAL-INTENT.md`
11. `sddk-evolution/INNOVATION-PULL-UP.md`
12. `roadmap/ROADMAP.md`
13. `uat/MASTER-UAT.md`

## What to implement first

Do not start with Monaco.

First prove this chain:

```text
direct SDDK core
   +
Vistalith event log
   +
Semantic World Graph
   +
Conversation Thread
   +
one LLM provider
   +
one native tool
   +
one C4/graph lens
   +
one VisualIntent
```

The first end-to-end UAT must demonstrate that a subject selected in chat, C4
and workflow views is the same semantic entity, and that a visual proposal can
be previewed before SDDK-governed work is created.
