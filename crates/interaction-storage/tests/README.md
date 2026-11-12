# interaction-storage tests

Run the package with its workspace CI features:

```sh
cargo nextest run --locked --profile ci -p interaction-storage
```

`integration/main.rs` gathers the remaining former external test root, `wal_mode`:
the opened interaction database uses the WAL journal mode. Library unit tests
remain colocated with their implementations. History unit cases prove that
retention reads durable settlements while the acknowledged cache remains
unchanged, and that inventory failures return errors. Schema cases prove that
the retained migration lineage, the durable-steps journal and the delivery
migrator's tables are admitted exactly, and that malformed files are refused
without rewriting.

Each aggregate declares shared helper implementations once. Existing fixture
directories remain in place. Only nonempty test categories are declared in
Cargo.toml; CI selects no separate e2e category for this package.

A former binary `<root>` and test `<case>` becomes its category binary and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
