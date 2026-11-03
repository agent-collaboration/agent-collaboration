# Automation storage tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p automation-storage
```

`integration/main.rs` gathers the 41 former external roots. The cases cover
storage initialization, durable schedules and wakeups, delivery transitions,
run preparation and recovery, replay, retention and history. Library unit tests
remain colocated with their implementations.

The aggregate declares the shared wake-push draft helper once. Case expansions
and fixture data stay in their existing directories. No empty e2e target is
declared because CI selects no separate e2e category for this crate.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
