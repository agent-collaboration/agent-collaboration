# Codex native integration tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p codex-native-integration
```

`integration/main.rs` gathers the 19 former external roots. Cases cover native
schema export, storage and validation; debug endpoints and profiles; executable
and desktop launch identity; callbacks, observations and connection events;
and validated input, resume, thread and rejection contracts. Library unit tests
remain colocated.

Case expansions and fixture data retain their existing directories. No shared
helper implementation needs hoisting, and CI selects no separate e2e category,
so only the integration target is declared.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
