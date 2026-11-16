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

- `owner_changes` and `backlog_boundaries` cover atomic backlog close/note/reference
  and change rows, replay, late-statement rollback, workspace and stored-value
  boundaries, and self-join roles with real post admission.

- `workspace_requests::mail`: central queue replay/expiry, typed senders and bodies,
  wake/automation lifecycle, finite and late schedules, actual injected-clock driver,
  interrupted alarm publication and earlier-deadline ordering, transaction rollback,
  timestamp bounds and sign-in stamp identity on startup. Fixtures seed the stamp;
  provisioning and signature verification remain slice 4.
- `workspace_requests::notices`: addressed and role activity joins session windows,
  stored shared epoch 15/25/55-minute cadences, cancellations, explicit non-session skips/inbox retention,
  workspace-turn races, independent uniqueness, before/after commit recovery,
  cross-workspace deadline isolation and position-based progress after clock rollback.

The existing direct-mail case also runs `mail::timing_contract`: five-minute and
seven-day admission boundaries, aligned relative schedules, immediate two-session
queues without delivery alarms, shared epoch slots across ten threads and two workspaces, pending
cadence changes and reopen, source-specific cancellation, stored-value rejection,
expiry, and idle/safety-alarm checks. It observes deliverability through the typed
non-destructive queue read and the actual injected-clock alarm driver. Link
handover and acknowledgements remain outside this package's timing proof.

The merged owner-change/drain case checks that backlog close and its reference
produce two consecutive changes, both are drained, and replay preserves the cursor.

- `organization_signin` (requires `test-support`, Unix): modern private sign-in
  routes through `OrganizationSignInLink`, real reqwest/ring verification and
  loopback JWKS publication, with the owning SQLite files and one `ManualClock`.
  Cases cover stamp initialization/repeats/renewals and competing writers,
  invalid and cold-damaged statements, retired-key refresh, floor delivery,
  signed audit reads and null workspace lookup, person admission and fresh-proof
  replay, pre-sample expiry and post-sample org-turn linearization, and the first
  real stamp activating a pending wake through the native alarm owner.
  Shared signers live in `collaboration-central-conformance`'s non-default
  `signin-test-support` feature. Setup cases exercise signed workspace creation,
  non-null lookup, byte-identical operation replay, shared canonical name uniqueness,
  atomic directory/ledger/audit writes, and stored stamp facts outside both time
  bounds without granting live authority.

- `organization_signin::machine_login`: the real private device/token decision
  routes, exact immutable three-field approval replies across reopen, both BA1
  grant orders, generation-stable active repeats, code and receipt boundaries,
  idle renewal/expiry, client/org/installation isolation and late-record rollback.
  Workspace rows are explicitly seeded only as directory preconditions; no
  approval, token, actor or change-record result is fabricated. Grant expectations
  name the fixed 600-second production policy. Owner revocation/ABA uses only the
  approved closed core test-support dispatcher, with real-store owner-origin
  multi-record rollback. No public revoke API or connection-close proof is implied.
  The invariant case enumerates six bounded three-login operation sequences,
  checking one-enrolled and the independent generation model after every step.
  Separate raw-row constraint probes carry the historical second-enrolled and
  duplicate-request/client engine checks, with fresh PKs, rollback snapshots and
  valid control inserts. These planted constraint rows are not login journeys.
  Stored-corruption cases require an empty 503 with unchanged owning rows, then
  restore the fixture rows and require a successful Grant.
  Observation cases read actual machine/approval projections, refuse corrupt
  generation or grant-time fields without writes, then repair only setup and
  require the original complete observation.

Ported from the paused reliability branch (alarm robustness):

- `workspace_requests::mail::alarm_isolation::corrupt_queued_message_does_not_block_due_wake`:
  an unreadable queued message stays untouched while the due wake fires and the
  alarm moves to the later wake; no timer attempt record is written.
- `workspace_requests::mail::alarm_admission`: corrupt terminal wake history does not
  block fresh mail, alarm recomputation or cold startup; an unreadable active due
  keeps unrelated mail and re-arms at the next five-minute mark; a shaped legacy wake
  outside current grid/horizon policy still fires once.
- `workspace_requests::mail::request_recovery`: warm Show, generic CreateWorkspace and
  exact replay each re-arm a physically missing alarm on the same runtime; a failed
  completion re-arm preserves the committed answer and its replay.
- `workspace_requests::mail::alarm_startup`: one organization with a corrupt identity
  does not block a healthy organization's host reopen, in either creation order.
- `workspace_requests::notices::drain_replay_isolation`: a committed post and its replay
  succeed when its drain is corrupt; the retry is the `legacy_workspace_drains` record.
- `workspace_requests::notices::workspace_outage`: an unavailable workspace store does not
  block another workspace's window or a due wake, and recovers without duplicates.
- `organization_signin::alarm_recovery`: the private HTTP digest route re-arms a missing
  alarm on the warm owner, and a failed re-arm keeps the digest answer.
