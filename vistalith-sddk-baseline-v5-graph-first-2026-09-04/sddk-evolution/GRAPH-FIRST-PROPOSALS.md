# Graph-First Proposals for SDDK — Focus-Preserving

SDDK already has graph-oriented concepts in planning, test impact, Decision Memory
and H9 Active Graph/Cockpit. Vistalith should exercise those surfaces and feed
back missing semantics.

## Possible future proposals

### SemanticSubjectRef
A generic cross-capability identity for requirement/work/test/evidence/decision
subjects if current IDs prove too fragmented.

### SemanticChangeProposal
Renderer-independent request describing desired system transformation before it
becomes governed work.

### ImpactAnalysis
Typed affected-subject graph with confidence/provenance/unknown edges.

This appears especially compatible with SDDK's existing:
`ActiveChangeSet → ProjectTestTopology → SUT Impact Graph → VerificationCapability`.

### CausalPath
A typed graph path for `why`, allowing reasons/evidence/decisions/events to be
rendered by any adapter.

### Advisory information class
Allow SDDK to explicitly distinguish inferred/proposed facts from authoritative
facts when such distinction is not already encoded.

### Pattern-based assurance
Use graph-shaped predicates for checks such as:
- accepted requirement has no verification evidence;
- changed SUT has stale evidence;
- decision's revisit condition became true.

## Rejected as SDDK concerns

- graph canvas;
- graph DB choice for Vistalith;
- LLM graph context;
- provider/model nodes;
- chat graph;
- MCP graph;
- renderer adapters.
