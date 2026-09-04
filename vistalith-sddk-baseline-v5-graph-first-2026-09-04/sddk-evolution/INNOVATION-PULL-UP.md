# Innovation Pull-Up Process

## Questions

For every significant Vistalith feature ask:

1. Does it make sense without a GUI?
2. Does it make sense without an LLM?
3. Is it useful from CLI/agents/humans/automation?
4. Does it improve decisions, workflow, evidence, policy, knowledge, verification
   or traceability?
5. Would keeping it only in Vistalith duplicate semantic truth?
6. Can it preserve SDDK determinism/authority?
7. Is there real UAT/evidence from Vistalith demonstrating the need?

## Classification

- `VISTALITH_ONLY`
- `SDDK_WATCH`
- `SDDK_SPIKE_CANDIDATE`
- `SDDK_PROPOSAL`
- `SDDK_ABSORBED`

## Required evidence for SDDK_PROPOSAL

- at least one concrete Vistalith UAT;
- demonstrated repeated semantic need;
- renderer/provider independence;
- no LLM dependency unless SDDK itself already needs the generic concept;
- clear authority owner;
- deterministic contract or explicit uncertainty model;
- migration/compatibility implications;
- proposed location in existing H0-H12, never a parallel roadmap.
