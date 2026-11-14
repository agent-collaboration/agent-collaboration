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
`support/workerd-bridge.mts` only starts the existing pinned loader, reports its
loopback origin and closes it; it has no business assertions or fabricated replies.

From the repository root:

```sh
pnpm --dir verification install --frozen-lockfile
worker-build crates/collaboration-central-cloudflare --dev --no-opt --out-dir build-test-support -- --features test-support,conformance
cargo nextest run -p collaboration-central-cloudflare --features test-support --test integration
```

Each case owns one workerd child and private SQLite persistence. Nextest reserves
the existing process-runtime group so these children retain per-case isolation
without competing for all of the host's memory. The bridge's imported loader keeps
the pinned Miniflare/workerd versions and refuses external fetches.

Cases cover admission/org/workspace boundaries, caller and actor identities, all
workspace rule families, scoped views/search/paging, persistent logs/redaction,
request replay/expiry, corruption/migration rollback, controlled time, held-turn
races and lost replies after actual apply. Person context is fixture-stamped;
person sign-in and Host enforcement remain C/slice 4. The existing generic storage
and driver cases stay in the verification package.

`workspace_requests::mail` and `::notices` exercise central queue, window, drain and
wake/automation state through real platform alarm callbacks. Test-only controls arm
storage's alarm and observe the callback's persisted receipt; cold cases require a
fresh owner and read the SQL organization stamp before any addressed warmup. Callback
entry/exit alarm readbacks distinguish platform dispatch from transaction rollback.
Fixture stamps do not establish provisioning or signature verification. Delivery,
link reconnect and acknowledgements remain later slices.
