# Reactive Behaviors

## Contract

A behavior declares:
- name/version;
- subscribed event types;
- optional graph pattern;
- required semantic view;
- determinism class;
- permissions;
- budget;
- output event/proposal types.

## Behavior categories

### ProjectionBehavior
Rebuild/update derived graph.

### ContextBehavior
Maintain context views for chat/agents.

### AdvisoryBehavior
Generate suggestions/hypotheses; never authoritatively mutate SDDK.

### VisualBehavior
Produce layout/annotation/lens projections.

### ObservationBehavior
Convert external runtime/tool/provider observations into normalized events.

### ValidationBehavior
Detect graph invariants, contradictions or missing provenance.

## RelationBehavior

A behavior may attach to relation kinds.

Examples:
- `depends_on`: propagate advisory impact;
- `verified_by`: invalidate a visual freshness projection;
- `contradicts`: surface conflict;
- `delegated_to`: update agent graph and ownership;
- `implements`: connect code changes to architecture subjects.

## Guardrail

A behavior cannot silently turn advisory state into authoritative SDDK state.
