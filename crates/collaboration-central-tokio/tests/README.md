# Central Tokio tests

Run this package's tests with nextest:

```sh
cargo nextest run --profile ci -p collaboration-central-tokio --features conformance
```

- `integration/main.rs` aggregates case modules covering native access,
  object storage, copy/registry/session behavior, process recovery and the
  composed board runtime, plus one-Fleet ownership, project boundaries and
  authenticated Push/status contracts. Each former Cargo root keeps its module namespace.
  `revocation_carryover` includes the shared Native/Worker O11(b) case:
  a committed revoked sender with a still-live session cannot admit an item
  or cancellation, change Fleet outbox count, or cause a recipient Push.
  `fleet_mail_observer` drives the shared owner-only custody observer through
  real admission and refusal, including payload presence, exact status/version,
  global count, UUIDv7 input and machine/unauthenticated caller rejection.
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
