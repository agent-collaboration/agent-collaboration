# Collaboration service tests

Run integration tests with Cargo nextest:

```sh
cargo nextest run --profile ci -p collaboration-service --features test-support
```

Cost proofs rely on one process per test, because `read_cost_counter` keeps
process-global counters and installs a process-global tracing subscriber.
Nextest preserves that isolation even though the cases share one executable.
Plain multi-case `cargo test` execution is not supported for this aggregate;
`cargo test --no-run` remains useful for measuring compilation.

- `integration/main.rs` is the single external test target. Each former Cargo
  test root retains its own module namespace. The cases exercise application
  assembly, board and thread APIs, admission and bounded read/write costs,
  native/provider delivery, subscriptions, schedules, wakes, and storage recovery.
- Subdirectories under `integration/` keep case-specific expansions and their
  fixture data beside the owning case module.
- `support/` contains shared fixtures. Reused helpers are declared once by
  `integration/main.rs`; case-specific helpers remain under their owning module.
  Helper unit tests are colocated in `*_test.rs` files.

This crate has no separate e2e category. Compiled CLI and Host end-to-end journeys
belong to their owning crates, so no empty e2e target is declared here.

Test identity mapping: a former binary `<root>` and test `<case>` becomes
binary `integration` and test `<root>::<case>`. Library test identities do not
change. Nextest scheduling and timeout overrides follow that same mapping.
