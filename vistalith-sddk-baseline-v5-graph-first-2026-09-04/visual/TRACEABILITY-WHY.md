# Traceability and Why

The SWG enables traversals such as:

```text
Requirement
 → Decision
 → Architecture
 → Code
 → Test
 → Evidence
```

and:

```text
Component
 → changed_by WorkItem
 → decided_by Decision
 → supported_by Evidence
```

## WhyQuery

Questions:
- why does this component exist?
- why is this action blocked?
- why was this test selected?
- why did this decision win?
- why does this relationship exist?
- why is this evidence stale?

The UI renders paths; generic causal semantics proven useful may be pulled up to
SDDK H9 proposals.
