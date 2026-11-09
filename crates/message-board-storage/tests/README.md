# message-board-storage tests

Run the package with its workspace CI features:

```sh
cargo nextest run --locked --profile ci -p message-board-storage
```

`integration/main.rs` gathers 3 test roots.
Cases cover durable cursor signing keys, actual indexed query plans for keyed record batches, and the update ledger in the Fleet database (never created by a migrating open, read without the store lock, every step recorded done in a store the Host creates). Library unit tests remain colocated with their implementations.

With `--features test-support`, the colocated notice-progress observer contract
checks canonical handled progress beside a live store, explicit missing state,
and invalid-row refusal. The observer opens read-only without creating or migrating
the database. Its feature is enabled only by test consumers.

Each aggregate declares shared helper implementations once. Existing fixture
directories remain in place. Only nonempty test categories are declared in
Cargo.toml; CI selects no separate e2e category for this package.

A former binary `<root>` and test `<case>` becomes its category binary and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
