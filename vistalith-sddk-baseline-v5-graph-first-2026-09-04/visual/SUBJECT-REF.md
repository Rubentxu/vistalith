# SubjectRef

```rust
struct SubjectRef {
    namespace: String,
    kind: SubjectKind,
    id: String,
    revision: Option<String>,
}
```

The concept may have a TypeScript mirror in the Vistalith protocol.

## Examples
- `sddk:work-item:TEST-MODEL-001`
- `arch:container:payment-service`
- `code:symbol:...`
- `visual:hypothesis:...`

## Cross-lens rule
Every lens maps renderer-native IDs to SubjectRefs.

Selection propagates SubjectRefs, not node IDs.
