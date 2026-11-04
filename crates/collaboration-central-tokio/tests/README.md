# Central Tokio tests

Run this package's tests with nextest:

```sh
cargo nextest run --profile ci -p collaboration-central-tokio --features conformance
```

- `integration/main.rs` aggregates 73 case modules covering native access,
  object storage, copy/registry/session behavior, process recovery and the
  composed board runtime. Each former Cargo root keeps its module namespace.
- `e2e/main.rs` aggregates the four roots selected by CI's e2e job:
  `fleet_client_cache`, `forwarded_posts`, `forwarding_real_timer` and
  `mailbox_rules`. This target requires `conformance`.
- The other test directories contain fixture modules and case expansions.
  Each aggregate declares its reused fixture modules once, preserving nested
  fixture ownership and each case's isolated server and storage.

CI intentionally also runs three e2e roots in the main workspace job. Only
`forwarding_real_timer` is excluded there. The aggregate selectors preserve
that existing overlap and the timer case's process-runtime reservation.

Test names map from binary `<root>` / test `<case>` to the appropriate aggregate
binary / test `<root>::<case>`. Library test identities do not change. Integration
and e2e cases run under nextest with one process per test.
