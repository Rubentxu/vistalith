# Patterns, Views and Frames

## GraphPattern

A typed query expressing a semantic shape.

Examples:
- requirement lacking evidence;
- decision supported by stale evidence;
- component affected by changed schema;
- work item blocked by unresolved human decision;
- agent conclusion contradicting another evidence-backed conclusion.

Start with a small Vistalith graph query DSL; do not implement full Cypher unless
real cases justify it.

## SemanticContextView

Bounded graph slice:
- root SubjectRefs;
- relation allowlist;
- depth;
- authority classes;
- recency;
- provenance detail;
- token budget;
- negative-knowledge inclusion.

This becomes a major input to the LLM Context Compiler.

## Frame

A bounded execution/exploration context:
- goal;
- subjects;
- constraints;
- budget;
- permitted tools;
- model profile;
- behavior set;
- branch/ref;
- expected outputs.

Frames are Vistalith agentic constructs. If generic lifecycle semantics emerge,
they can later be evaluated for SDDK pull-up.
