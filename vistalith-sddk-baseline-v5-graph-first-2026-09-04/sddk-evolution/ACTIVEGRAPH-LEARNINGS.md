# ActiveGraph Learnings Potentially Relevant to SDDK

The following ActiveGraph ideas deserve observation because they align with
existing SDDK directions:

## Event-sourced projection
Strong match with Event Ledger + reconstructible projections.

## Fork-and-diff
Strong match with Decision Memory and Decision Lab.

## Patches with optimistic concurrency
Potentially useful for semantic proposals and stale-revision handling.

## Pattern subscriptions
Potentially useful for assurance, invariants and bounded reactive rules.

## Failure as event
Potentially useful where SDDK still leaks exception-only semantics into durable
runtime behavior.

## Views
Potentially useful to make context/projection queries explicit and bounded.

## Relation behavior
Interesting but dangerous: only pull up deterministic relationship semantics
that belong to SDDK. Do not introduce a generic reactive agent framework into
the kernel.

## Packs
SDDK already has pack concepts. Compare composition lessons, but do not copy
ActiveGraph pack semantics wholesale.
