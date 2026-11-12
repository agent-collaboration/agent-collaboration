# Central Core tests

Run with the conformance feature enabled in workspace CI:

```sh
cargo nextest run --locked --profile ci -p collaboration-central-core --features conformance
```

`integration/main.rs` gathers the 10 remaining former external roots. Cases exercise
Central Core's identity, access, admission and wire contracts. Library unit
tests remain colocated with their implementations.

Each aggregate declares shared helper implementations once. Case expansions,
contract fixtures and other data retain their existing directories. CI selects
no separate e2e category, so only the integration target is declared.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.

The weak-key access fixture lives in `fixtures/` beside the integration target.
