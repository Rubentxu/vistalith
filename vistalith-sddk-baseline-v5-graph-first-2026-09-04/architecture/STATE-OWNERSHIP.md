# State Ownership

## SDDK authoritative
- planning;
- workflows/runs;
- SDDK decisions;
- SDDK evidence/receipts;
- SDDK gateway decisions;
- Decision Memory;
- SDDK knowledge;
- SDDK UAT.

## Vistalith authoritative
- conversation/session/thread/turn/item;
- provider/model profiles;
- MCP configuration;
- Vistalith agent profiles;
- visual workspace and drawings;
- VisualIntent drafts;
- graph annotations created in Vistalith;
- LLM usage/traces;
- UI layouts/preferences.

## Vistalith derived
- graph projections of SDDK facts;
- architecture/code/test cross-links inferred from source analysis;
- impact hypotheses;
- visual projections;
- cached graph paths.

## Invariant

Deleting a derived graph projection cannot destroy SDDK truth.

Deleting Vistalith-owned event history may destroy Vistalith chat/visual truth,
so that event log is durable and backed up separately from reconstructible
projections.
