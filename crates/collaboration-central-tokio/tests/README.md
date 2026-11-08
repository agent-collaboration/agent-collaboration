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

## Test folder ownership

- `integration/copy_owner_health`: integration modules, moved from `copy_owner_health/`.
- `integration/copy_receiver_fixture`: integration modules, moved from `copy_receiver/`.
- `integration/copy_receiver_session`: integration modules, moved from `copy_receiver_session/`.
- `integration/directory_session`: integration modules, moved from `directory_session/`.
- `e2e/fleet_client_cache`: e2e modules, moved from `fleet_client_cache/`.
- `integration/fleet_notice_outbox`: integration modules, moved from `fleet_notice_outbox/`.
- `integration/fleet_requests`: integration modules, moved from `fleet_requests/`.
- `integration/fleet_storage`: integration modules, moved from `fleet_storage/`.
- `e2e/forwarded_posts`: e2e modules, moved from `forwarded_posts/`.
- `integration/machine_enrollment`: integration modules, moved from `machine_enrollment/`.
- `integration/machine_notices`: integration modules, moved from `machine_notices/`.
- `support/mail_submit_http`: e2e and integration modules, moved from `mail_submit_http/`.
- `support/native_http`: e2e and integration modules, moved from `native_http/`.
- `integration/normal_redeploy`: integration modules, moved from `normal_redeploy/`.
- `integration/object_bootstrap`: integration modules, moved from `object_bootstrap/`.
- `integration/owner_registry`: integration modules, moved from `owner_registry/`.
- `integration/owner_takeover_recovery`: integration modules, moved from `owner_takeover_recovery/`.
- `integration/process_restart`: integration modules, moved from `process_restart/`.
- `integration/session_credentials`: integration modules, moved from `session_credentials/`.
- `integration/session_move`: integration modules, moved from `session_move/`.
- `support/session_publication`: e2e and integration modules, moved from `session_publication/`.
- `integration/step_transactions`: integration modules, moved from `step_transactions/`.
- `integration/transfer_boundaries`: integration modules, moved from `transfer_boundaries/`.
- `integration/transfer_missing_copy`: integration modules, moved from `transfer_missing_copy/`.

Existing `support/` and fixture data stay in place. Module names, case identities
and process selectors are unchanged.
