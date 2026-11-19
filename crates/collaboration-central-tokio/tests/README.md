# Central Tokio tests

Run this package's tests with nextest:

```sh
cargo nextest run --profile ci -p collaboration-central-tokio --features test-support
```

Integration cases run under nextest with one process per test.
`integration/main.rs` (binary `integration`, requires the `test-support` feature)
proves this host's side of the storage boundary on real SQLite files (Verification
O1), with a test-only `widgets` schema from `collaboration-central-core`'s
`test-support` feature and one injected clock that moves only when a case moves it:

- `central_object_batches`: one object's file — a later guard failure rolls the
  whole batch back; Unique, ForeignKey, NotNull and boolean CHECK errors map to
  their `ConstraintKind` and roll back; `SetFromQuery` keeps the alarm from the
  batch's own rows, a rolled-back batch leaves it, and NULL clears it.
- `workspace_store_calls`: central's calls to a workspace store, made as
  `TokioWorkspaceStores` makes them — ids minted from the held clock keep mint
  order in one batch; a migration failing on its second statement leaves no
  partial schema and `_sqlx_migrations` unchanged.
- `request_pause_seam`: the pause points and write turns — a second request waits
  for the turn and reads the first one's write; a crash at a held point keeps
  exactly what the store had applied.

Run: `cargo nextest run -p collaboration-central-tokio --features test-support`.

- `alarm_publication_order`: holds an older real commit before alarm publication,
  then forces a newer batch to queue or expose the forbidden commit order. It covers
  set/set, clear/set and set/clear, with alarm readback compared to stored rows.

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
- `workspace_requests::mail::engine_event_support`: shared native helpers for the V6, V9
  and V12 checks (named times from 10:00:00, the held-clock event driver, wake, window
  and probe fixtures).
- `workspace_requests::mail::engine_publication_failures`: V6 on the native host; a
  post-commit publication failure keeps the rows and the previous alarm, the upper range
  has no mark and arms nothing, a restart mid fan-out redoes nothing, and a dropped alarm
  is re-published by the next request.
- `workspace_requests::mail::engine_large_batch`: V9 on the native host; 1,000 due probe
  instances in four capped batches 30 s apart, earliest first and once each, with at most
  eight units in flight; prints batch timings as measurement only.
- `workspace_requests::mail::engine_timing_races`: V12 races on the native host; a
  publication built before a newer claim uses the newer floor and backlog, a claim waits
  out a decision's publication, simultaneous events run one due batch, notice units get no
  look-ahead, and a changed organization stamp refuses every event.
- `workspace_requests::engine_v2_support`: shared native V2 helpers (failure record
  reads, mark arithmetic, wake and automation requests, and a `tracing` capture of the
  native `EngineItemFailed` log lines).
- `workspace_requests::mail::engine_isolation_v2`: V2 on the native host; a corrupt wake
  (target, due time, state) or automation (timing rule) beside healthy ones fails alone
  at count 1 with the next mark, is observed, retires on the next pass and is never
  visited during the following hour.
- `workspace_requests::notices::engine_notice_units`: V2 on the native host; a failing
  drain arms its next try on a mark, never its past drain time; the drain unit's control
  row gates due batches and request drains across a restart; a failing request drain
  records against the unit and keeps its committed answer.
- `workspace_requests::notices::engine_drain_origins`: V2 error origin on the native host;
  workspace-store outage, central outage, rejected, uncertain, unique-constraint and
  guard-failed drain batches each take their own outcome, and the two-workspace mixes keep
  CentralDown first and a committed workspace committed.
- `organization_signin::engine_admin_support`: an organization with a workspace, today's
  timers and the four engine admin routes over the real sign-in link with person proofs.
- `organization_signin::engine_admin`: V13 on the native host; EngineHealth lists every
  kind, the retired list pages in attempt order, retry and discard with their refusals,
  replays and admission.
- `organization_signin::engine_unit_revival`: V2 notice units and `engine_source` on the
  native host; each unit recovers by itself, restarts its 60-day period, retires at 60
  days, stays stopped and returns only by EngineRetry; a retired source is never read.

## Machine link and self-hosted central

The `workspace_link` integration module opens actual loopback WebSockets
using the modern workspace host. It exercises signed admission, renewal,
batch, immediate request and reply correlation, delivery replay, and private
signed revocation of an idle link. The fixture owns the host, private Unix
listener, public socket and signing keys for each case.

`codex-router central serve --config FILE` starts the same link and a private
`signin.sock`. In the JSONC file, `listen` is the TCP address and port the
carrier should reach. Configure a private interface or loopback address for
that bind. `root` is an absolute private directory owned by the running
account. The machine JWT audience is configured separately under
`signIn.machineAudience`. The CLI integration target's `central_serve` case
checks the route boundary and shutdown of the compiled command.
