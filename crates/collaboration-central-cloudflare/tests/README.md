# collaboration-central-cloudflare test contracts

This package has one native Rust integration target, `integration/main.rs`,
requiring `test-support`, and no e2e target. Unit tests stay colocated with their
implementation (`src/*_test.rs`).

- `public-template.test.mjs`: the public deployment template keeps its placeholders
  and binds `FLEET` and `WORKSPACE_STORE` to SQLite Durable Object classes. Run from
  the root with `node --test crates/collaboration-central-cloudflare/tests/public-template.test.mjs`.
- The non-default `test-support` feature compiles the storage boundary's workerd
  case routes (`/test-support/...`), the moved SQLx driver's scenarios
  (`/test-support/driver/`) and their `StorageCaseObject` and `DriverScenarioObject`
  classes into a
  separate test bundle (`build-test-support/`). The cases themselves live in
  `verification/targets/workerd-central-storage.test.ts`; the production bundle check
  refuses a bundle that carries any of these routes.

The O1 bundle is built with `--features test-support,conformance`. Its actual-object
clock case calls the real `FleetObject` clock control and `IdSource`, checks held
and advanced UUIDv7 milliseconds, and fails if the route is absent. The route is
compiled only when both test features are enabled and is rejected by the production
bundle marker check.

## Modern workspace request integration tests

`integration/main.rs` is the business oracle. Its Rust cases drive typed semantic
requests, fixture controls and actual owning-store reads through HTTP to the compiled
Worker's `CentralObject` and `WorkspaceStoreObject`. The TypeScript bridge at
`support/workerd-bridge.mts` starts the existing pinned loader, reports its
loopback origin, forwards fixture clock/observation commands and closes it;
Rust owns business assertions and signing.

Ordinary bridge mode installs no launcher clock binding, preserving production
request-entry concurrency for the existing Worker cases. Launcher controls require
`--launcher-time-ms <milliseconds>` before readiness. Only that mode installs the
clock binding; each object instance samples it on first touch, then receives live
time pushes. The first-touch outside await is a named test-only difference; it is
absent from ordinary mode and production. A cold object samples the current cell.
Invalid time input is refused before any effect. A mid-push transport failure can
leave time partly applied, fails the run, and has no retry or rollback machinery.

From the repository root:

```sh
pnpm --dir verification install --frozen-lockfile
worker-build crates/collaboration-central-cloudflare --dev --no-opt --out-dir build-test-support -- --features test-support,conformance
cargo nextest run -p collaboration-central-cloudflare --features test-support --test integration
```

Each case owns one workerd child and private SQLite persistence. Nextest reserves
the existing process-runtime group so these children retain per-case isolation
without competing for all of the host's memory. The bridge's imported loader keeps
the pinned Miniflare/workerd versions. Its default mode refuses external fetches.

Cases cover admission/org/workspace boundaries, caller and actor identities, all
workspace rule families, scoped views/search/paging, persistent logs/redaction,
request replay/expiry, corruption/migration rollback, controlled time, held-turn
races and lost replies after actual apply. Person context is fixture-stamped;
the verified-person sign-in cases use their separate private-link suite below.
Machine-token verification and Router Host admission retain their later-slice
boundaries. The existing generic storage and driver cases stay in the verification
package.

`organization_signin::launcher_controls` drives the actual bridge child over stdin
and stderr: unknown-org observation stays read-only, signed provisioning and renewal
read the shared clock for live and future organizations, and a new bridge process
starts with empty persistence. Raw fixture statements are compared without dumping
their bytes into assertion failures. The existing integration command runs it.
Its machine observation cases drive real approval, supersession, lapse and idle
expiry, then compare both machines and all creation-ordered approval rows over
the same IPC. The machine-login observation case rejects damaged generation and
grant timestamps with unchanged owner snapshots and repaired positive controls.

`workspace_requests::mail` and `::notices` exercise central queue, window, drain and
wake/automation state through real platform alarm callbacks. Test-only controls arm
storage's alarm and observe the callback's persisted receipt; cold cases require a
fresh owner and read the SQL organization stamp before any addressed warmup. Callback
entry/exit alarm readbacks distinguish platform dispatch from transaction rollback.
Fixture stamps do not establish provisioning or signature verification. Delivery,
link reconnect and acknowledgements remain later slices.

The existing direct-mail case also runs `mail::timing_contract`, covering the
five-minute grid, seven-day horizon/expiry, immediate two-session queues without delivery alarms,
shared epoch 15/25/55-minute slots across ten threads and two workspaces, pending policy
changes/reopen, source-specific cancellation and corruption. Its test control
dispatches the real platform callback only when the retained alarm is due on the
injected clock, and records callback count. Empty/later alarms cannot fabricate a
callback. This proves deadline selection and callback effects; it does not advance
the platform wall clock or establish C/D link handover. These assertions reuse
the existing case's workerd process.

`owner_changes` and `backlog_boundaries` drive backlog close/note/reference and
change-row atomicity, replay, late-statement rollback, workspace/stored-value
boundaries, and self-join roles through actual Worker requests and store readback.

The merged owner-change/drain case checks that backlog close and its reference
produce two consecutive changes, both are drained, and replay preserves the cursor.

## Verified organization sign-in

`workspace_requests::organization_signin` sends actual HTTP requests through a
gateway service binding to a separately compiled private-link Worker, then to the
public Worker's owning central object. Real Worker fetch and WebCrypto verify the
Rust fixture's published keys. Only one configured HTTPS JWKS URL maps to its
owned loopback fixture; Rust owns signing, key rotation and all business assertions.

```sh
worker-build crates/collaboration-central-cloudflare --dev --no-opt --out-dir build-test-support -- --locked --features test-support,conformance
worker-build crates/collaboration-central-cloudflare --dev --no-opt --out-dir build-link-test-support -- --locked --features link,test-support,conformance
cargo nextest run --locked -p collaboration-central-cloudflare --features test-support,conformance -E 'test(organization_signin::)'
```

Cases cover stamp and record atomicity, expiry and predecessor checks, current
revision floors, signed person reads, fresh proofs, causal races, cold reopen,
query-derived alarm activation and safe refusal observations. Public-path refusal
uses the existing `401` edge contract. Unknown private paths return `404`.
Setup cases exercise real signed create, non-null lookup, byte-identical replay,
canonical name collisions, atomic directory/ledger/audit writes and facts-only
digest reads outside both time bounds.

`workspace_requests::organization_signin::machine_login` drives the actual private
Worker decision binding with exact approval snapshots, BA1 grant ordering and
generation checks, code/receipt deadlines, live renewal and idle expiry, client/
org/installation isolation, and genuine late-record transaction failures.
Only workspace presence is seeded as a labelled precondition; approval/token
effects and audit rows come from the routes. Literal Grant expectations use the
fixed 600-second production policy. Revocation/ABA uses the granted `machine-login-revoke`
control at the existing Worker test-support dispatcher, with real owner-origin
withdrawal records and final-record rollback. It is not fixture SQL revocation,
a production revoke route, or proof of session/connection-close delivery.
The invariant case enumerates six bounded three-login operation sequences and
checks active-row cardinality and its independent generation model after every
step. Engine-only constraint probes separately reject a second enrolled row and
duplicate request/client values under fresh PKs, then accept valid full-row
controls. Planted constraint rows are labelled storage proof, not route effects.
Stored-corruption cases require an empty 503 with unchanged owning rows, then
restore the fixture rows and require a successful Grant.

`workspace_requests::workerd_host::tests` also checks the fixture's control HTTP
connection policy against real workerd. A byte-transparent relay observes upstream
idle EOF and forces its ordering against one next clock POST. Each call runs once;
the oracle uses connection events, without sleeps or relaxed deadlines.

`workerd_host::lifecycle_tests` checks real parent SIGKILL and panic, launcher and
legacy-owner panic, the inherited nextest group and real bridge EOF disposal.
Build the legacy bundle with `worker-build crates/collaboration-central-cloudflare
--dev --no-opt -- --locked --features conformance-faults` before these cases;
the Worker CI job already builds it for its legacy scenarios.
The bridge observes EOF as disposal; Rust's sole child owner waits and reaps.
The harness never kills
only the bridge; nextest bounds the whole test process group if disposal stalls.
No nested bridge process group escapes nextest's termination.

Standalone modern and legacy bridges remove Miniflare’s unawaited signal hooks at their one constructor and await disposal before signal exit. In-process TypeScript loaders keep their existing multi-runtime policy. `verification/targets/workerd-bridge-lifecycle.test.ts` permanently holds real SIGTERM disposal at an IPC gate, checks retained listener behavior and child exit, and checks cleanup before a missing-hook refusal. See [the pinned upstream workaround](../../../docs/upstream-workarounds/2026-11-14-miniflare-sigterm-unawaited-dispose.md) for scope and removal evidence.

`support/lifecycle-nextest.toml` belongs only to explicit lifecycle negative
controls. It does not alter normal CI deadlines. The `WORKERD_LIFECYCLE_CHILD`
timeout and leak modes require an operator to retain the emitted exact child PIDs,
check their OS exit and settle the deliberately leaked PID immediately. They are
ignored in ordinary suites. Run the owned negative-control driver from the root:

```sh
pnpm --dir verification exec node tooling/workerd-lifecycle-negative-controls.ts
```

It runs the explicit ignored timeout/leak selectors, fails on missing readiness
or zero matches, and observes workerd exit after timeout. The bridge may remain
an init-owned zombie after group termination. The driver requests SIGTERM for
its deliberate plain Node leak witness and records its PID; an exact OS exit
readback is required before reporting that witness settled.
Worker CI records nextest output in the job log and fails on a
genuine `LEAK` result, including a test that nextest otherwise calls successful.
An always-run `pgrep -x workerd` gate also rejects workerd-only orphans whose
output pipes belong to Miniflare and cannot produce nextest's `LEAK` result.

## Ported alarm-robustness cases

These cases come from the paused reliability branch and mirror the native ones:

- `workspace_requests::mail::alarm_isolation::corrupt_queued_direct_message_does_not_block_timers`:
  an unreadable queued direct message stays untouched while due and later wakes fire
  on the actual platform alarm; no timer attempt record is written.
- `workspace_requests::mail::alarm_admission`: corrupt terminal wake history does not
  block fresh mail, alarm repair or the cold alarm; an unreadable active due keeps
  unrelated mail and re-arms at the next five-minute mark; a shaped legacy wake
  outside current grid/horizon policy still fires once.
- `workspace_requests::mail::request_recovery`: warm Show, generic CreateWorkspace and
  exact replay each re-arm an alarm removed with the `raw-drop-alarm` control; a
  failed completion re-arm preserves the committed answer and its replay.
- `workspace_requests::notices::drain_replay_isolation`: a committed post and its replay
  succeed when its drain is corrupt; the retry is the `legacy_workspace_drains` record.
- `workspace_requests::notices::workspace_outage`: an unavailable workspace store does not
  block another workspace's window or a due wake, and recovers without duplicates.
- `workspace_requests::organization_signin::alarm_recovery`: the private digest binding
  re-arms a missing alarm on the warm owner, and a failed re-arm keeps the answer.
