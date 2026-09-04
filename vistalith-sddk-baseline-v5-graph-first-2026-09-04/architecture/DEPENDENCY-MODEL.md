# SDDK Dependency Model

## Local development

Sibling checkout:

```text
dev/
├── software-development-decision-kernel/
└── vistalith/
```

Use direct `path` dependencies during co-development.

## CI/release

Pin all SDDK crates to the exact same git tag/commit or published release.

Never mix SDDK crate revisions.

## Upgrade policy

An SDDK upgrade is a first-class Vistalith dependency upgrade:
1. update pin;
2. compile;
3. run contract/graph projection tests;
4. run master UAT;
5. inspect semantic diff;
6. accept or revert.

## No compatibility façade

Compile errors are useful evidence of real coupling. Do not hide them behind a
generic SddkPort.
