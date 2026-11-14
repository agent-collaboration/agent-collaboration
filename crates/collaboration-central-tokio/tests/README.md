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
  It also runs the shared eleven-branch mutation refusal scenario.
  `revocation_clock` holds the real Fleet transaction across enrollment expiry.
  `revocation_writer_recovery` joins the terminal pump and writer runner, then
  recovers retained registration, renewal and seed calls on a fresh session.
  `fleet_mail_reports` preserves the revoked-recipient Report exception with
  and without a prior handoff witness.
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
`integration/main.rs` (binary `integration`, requires the `test-support` feature)
proves this host's side of the storage boundary on real SQLite files (Verification
O1), with a test-only `widgets` schema from `collaboration-central-core`'s
`test-support` feature and one injected clock that moves only when a case moves it:

- `central_object_batches`: one object's file — a later guard failure rolls the
  whole batch back; Unique, ForeignKey, NotNull and boolean CHECK errors map to
  their `ConstraintKind` and roll back; `SetFromQuery` keeps the alarm from the
  batch's own rows, a rolled-back batch leaves it, and NULL clears it.
- `workspace_store_calls`: `CentralPlatform::call_store` — ids minted from the held
  clock keep mint order in one batch; a migration failing on its second statement
  leaves no partial schema and `_sqlx_migrations` unchanged.
- `request_pause_seam`: the pause points and write turns — a second request waits
  for the turn and reads the first one's write; a crash at a held point keeps
  exactly what the store had applied.

Run: `cargo nextest run -p collaboration-central-tokio --features test-support`.
The kept enrollment, Access and session-upgrade behavior is proven by
`collaboration-central-core`'s tests.

- `alarm_publication_order`: holds an older real commit before alarm publication,
  then forces a newer batch to queue or expose the forbidden commit order. It covers
  set/set, clear/set and set/clear, with alarm readback compared to stored rows.
- `src/native_objects_test.rs`: controls the real object clock and checks UUIDv7
  milliseconds minted through that same object, including a later clock advance.

- `workspace_requests`: the modern org-scoped request owner on approved central and
  workspace SQLite schemas. Cases cover live credential allowance, org isolation,
  pure-rule dispatch, views/search/inbox/read marks, actor identities, atomic role
  replacement, dependency walks, status closure, reference/link/backlog/log rows,
  cached success/refusal/no-op and 24-hour expiry, corruption rejection, clock-behind
  reopen, and causal write-turn races. Fixtures own their temporary host roots;
  fault writes use the actual owning file through the existing test-support store.
