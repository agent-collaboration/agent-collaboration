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
Machine login and Router Host enforcement retain their later-slice boundaries. The existing generic storage
and driver cases stay in the verification package.

`organization_signin::launcher_controls` drives the actual bridge child over stdin
and stderr: unknown-org observation stays read-only, signed provisioning and renewal
read the shared clock for live and future organizations, and a new bridge process
starts with empty persistence. Raw fixture statements are compared without dumping
their bytes into assertion failures. The existing integration command runs it.

`workspace_requests::mail` and `::notices` exercise central queue, window, drain and
wake/automation state through real platform alarm callbacks. Test-only controls arm
storage's alarm and observe the callback's persisted receipt; cold cases require a
fresh owner and read the SQL organization stamp before any addressed warmup. Callback
entry/exit alarm readbacks distinguish platform dispatch from transaction rollback.
Fixture stamps do not establish provisioning or signature verification. Delivery,
link reconnect and acknowledgements remain later slices.

The existing direct-mail case also runs `mail::timing_contract`, covering the
five-minute grid, seven-day horizon/expiry, exact two-session ready batches,
15/25/55-minute cadences across ten threads and two workspaces, pending policy
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
Create-workspace effects, non-null lookup and machine-login approval replay remain
held; independent read admission does not substitute for those effects.

`workspace_requests::workerd_host::tests` also checks the fixture's control HTTP
connection policy against real workerd. A byte-transparent relay observes upstream
idle EOF and forces its ordering against one next clock POST. Each call runs once;
the oracle uses connection events, without sleeps or relaxed deadlines.
