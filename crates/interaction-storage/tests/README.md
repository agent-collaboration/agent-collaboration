# interaction-storage tests

Run the package with its workspace CI features:

```sh
cargo nextest run --locked --profile ci -p interaction-storage --features runtime-test-support
```

`integration/main.rs` gathers 9 former external test roots.
Cases cover durable pushes, wake handoff, subscription progress, recovery, and database journal mode. Library unit tests remain colocated with their implementations.

Each aggregate declares shared helper implementations once. Existing fixture
directories remain in place. Only nonempty test categories are declared in
Cargo.toml; CI selects no separate e2e category for this package.

A former binary `<root>` and test `<case>` becomes its category binary and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
