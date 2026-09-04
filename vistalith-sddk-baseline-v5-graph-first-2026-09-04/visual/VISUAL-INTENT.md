# Visual Intent

## Flow

```text
VisualGesture
 → VisualIntentDraft
 → semantic resolution
 → SemanticChangeProposal
 → impact preview
 → explicit user/agent promotion
 → SDDK decision/workflow if governed
```

## Example
Draw a broker between Checkout and Orders.

The result is not a file edit. It is a typed proposal:
- subjects;
- proposed node/relation changes;
- assumptions;
- base graph/SDDK revision;
- expected outcome;
- risks;
- provenance from canvas shapes.

## Staleness
If base revision changes, preview becomes stale.

## Safety
No ambiguous drag/drop directly executes a host or SDDK effect.
