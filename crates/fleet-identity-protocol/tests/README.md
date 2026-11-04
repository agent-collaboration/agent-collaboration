# fleet-identity-protocol tests

Run the package with its workspace CI features:

```sh
cargo nextest run --locked --profile ci -p fleet-identity-protocol
```

`integration/main.rs` gathers 6 former external test roots.
Cases cover identity values, connection and registration wires, remote starts, activity, and launch validation. Library unit tests remain colocated with their implementations.

Each aggregate declares shared helper implementations once. Existing fixture
directories remain in place. Only nonempty test categories are declared in
Cargo.toml; CI selects no separate e2e category for this package.

A former binary `<root>` and test `<case>` becomes its category binary and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
