# Vistalith Context Compiler

## Inputs
- current Thread/Turn;
- selected SubjectRefs;
- SemanticContextView;
- direct SDDK current state;
- files/code snippets;
- decisions/evidence;
- recent tool results;
- negative knowledge;
- model context budget.

## Output
`CompiledModelContext` with provenance map.

## Graph advantage

Context is selected through semantic relationships rather than "nearest files".

Example:

`@PaymentService` can pull:
- implementation symbols;
- callers/dependencies;
- ADRs;
- active work;
- tests/evidence;
- recent decisions;
- current agents;
- relevant risks.

## Explainability

The UI can show "why this context item was included".
