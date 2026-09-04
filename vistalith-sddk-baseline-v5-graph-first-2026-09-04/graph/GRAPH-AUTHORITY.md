# Graph Authority Model

## authoritative
Owned by its domain authority:
- SDDK WorkItem;
- SDDK decision;
- accepted Vistalith conversation item;
- user-created visual note.

## derived
Reconstructible:
- architecture inferred from code;
- impact edge;
- traceability path;
- C4 projection.

## advisory
Hypothesis/proposal:
- agent-suggested dependency;
- what-if architecture;
- VisualIntent draft;
- predicted risk.

## ephemeral
UI/runtime-only:
- hover;
- viewport;
- current drag;
- transient streaming partial.

## Promotion

Promotion never means simply changing a flag.

```text
advisory candidate
    ↓ validate/evaluate
promotion request
    ↓ owning authority
accepted object/decision/work
    ↓ new authoritative fact
```

The advisory origin remains linked by provenance.
