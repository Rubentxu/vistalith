# Impact Visualization

Input:
- code change;
- architecture change;
- SemanticChangeProposal;
- SDDK WorkItem.

Output graph:
- directly changed subjects;
- transitive affected subjects;
- affected tests;
- stale evidence;
- decisions potentially invalidated;
- deployment/runtime boundaries;
- confidence/provenance.

Unknown impact is represented explicitly.

Impact discovered in Vistalith is advisory unless SDDK itself owns the
corresponding semantic analysis.
