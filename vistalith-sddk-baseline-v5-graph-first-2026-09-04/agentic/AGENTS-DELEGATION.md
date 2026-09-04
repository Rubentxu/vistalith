# Agents and Delegation

## Vistalith Agent
- role;
- instructions;
- model profile;
- tools;
- MCP scope;
- permissions;
- context view policy;
- budget;
- expected structured outputs.

## Delegation
Represent as graph subjects and relations:
- `delegated_to`;
- `depends_on`;
- `contributes_to`;
- `contradicts`;
- `supports`.

## Outputs
Agents should return:
- findings;
- evidence refs;
- assumptions;
- uncertainty;
- alternatives;
- risks;
- SemanticChangeProposal;
- VisualProposal where useful.

## SDDK
When an agent is executing governed SDDK work, SDDK role/workflow/authority
semantics remain binding.
