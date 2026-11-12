# Collaboration service tests

Run integration tests with Cargo nextest:

```sh
cargo nextest run --profile ci -p collaboration-service --features test-support
```

Nextest runs each case in its own process.

- `integration/main.rs` is the single external test target. Each former Cargo
  test root retains its own module namespace. The cases exercise application
  assembly, native and provider session control, ACP listeners, session delivery
  routes and presence, inventories, the lifecycle journal, approvals, questions
  and provider operations.
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
