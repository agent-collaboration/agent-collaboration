# Lifecycle observation tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p lifecycle-observation
```

`integration/main.rs` gathers the 14 former external roots. The cases exercise
lifecycle observation and its journal, including migration, retention and
recovery contracts. Library unit tests remain colocated.

Case expansions and fixture data retain their existing directories. No shared
helper implementation needs hoisting, and CI selects no separate e2e category,
so only the integration target is declared.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
