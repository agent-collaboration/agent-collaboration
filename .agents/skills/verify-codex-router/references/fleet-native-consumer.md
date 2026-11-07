# Fleet native consumer journeys

Use the real `fleet-native-services` consumer fixtures for service, transport,
lease, copy, delivery and restart behavior. The route is the integration/e2e
aggregates, with same-package child binaries and the repository signing runner.
A source count or a successful build leaves runtime coverage open.

The adopted preservation baseline is agent-fleet-sync
`b9470dcbfc177afc061203365b6bd6edbec35aa1`, `tests/consumer`, plus the caller's
approved adaptations. The destination mapping is
`crates/fleet-native-services/tests/support/consumer-cases.json`: **97 moved
cases, 15 integration and 82 e2e**. Preserve the existing destination cases and
the additional native query-description check separately. This is local native
Turso/WebSocket proof; workerd and hosted sessions have different boundaries.

## Declare and run

Declare the immutable source head, features, normal `target/debug`, signing
configuration and an exclusive build/observation window. Keep evidence in a
fresh task directory under `tmp/`, outside every build output. From the root:

```sh
git rev-parse HEAD
git status --short
df -h /System/Volumes/Data
env -u GITHUB_ACTIONS CARGO_BUILD_JOBS=2 cargo nextest list --locked -p fleet-native-services --features test-support,test-fault --test integration --test e2e --message-format json
env -u GITHUB_ACTIONS CARGO_BUILD_JOBS=2 cargo nextest run --locked -p fleet-native-services --features test-support,test-fault --test integration --test e2e --test-threads 1 --status-level all --final-status-level all
```

Stop before a build below 30 GiB available. The features are non-default and
required for these fixture targets; their omission cannot establish coverage.
Use `.cargo/config.toml` → `scripts/cargo_debug_signing_runner.sh` on macOS.
Never bypass it by executing an unsigned child. The runner signs the profile's
bin targets before nextest launches tests. Both `fleet-sync-test-consumer` and
`delivery_node` are built by this package, so `CARGO_BIN_EXE_*` identifies them
without guessed executable paths. Record their hashes and verify their debug
signatures when the assignment requires executable provenance.

Reconcile discovered and executed identities with every mapped row. A case
filtered out, compiled out, ignored unexpectedly or selected zero times is
missing proof. Keep subcases inside their owning Rust case; do not count wire
requests as additional tests. Reuse a completed full run for a focused journey
only after source/configuration applicability is established.

## Readiness, outcome and cleanup

`tests/support/process/mod.rs` starts the compiled service, requires its
PID-correlated `READY` line and authenticates an actual matching Welcome.
`tests/support/consumer/server.rs` opens and serves the real `FleetService`.
The tests own loopback ports, Turso files, sockets, pumps and child shutdown.
A listening port or `READY` alone does not prove authenticated session readiness.

Read each case's protocol answer together with its durable rows, receipts,
positions, identities and reopen assertions. Expected behavior comes from the
baseline and adopted contract, rather than the current answer. The existing
cases cover:

- Correlated malformed bodies with same-session recovery; malformed envelopes
  close without routing: [frame shapes](fleet-frame-shapes.md).
- Holder/capability refusals, frozen admission, report/Welcome ordering and
  replacement/restart: `native_target_capability`, `native_schema_admission`,
  `native_schema_publication`, `native_client_readiness`.
- Real-copy containment, committed reply loss, same-piece reconciliation,
  seed pinning, claim/move retry and filesystem publication. Keep callback
  faults and scripted holder answers distinct from physical I/O failure or a
  real writer producer.
- Delivery custody, loss/restart, expiry, status-owner corruption and storage
  retry. A healthy readback must be nonempty and retain the exact item, digest,
  status and version. `delivery_storage_retry` uses the actual service tracing
  target, `fleet_native_services`, to observe its operation-owned failure.
- Lease withdrawal/expiry and stale-holder renewal. Injected writer and
  service time must share one time domain, including renewal request-started
  timestamps. Keep every original assertion. The registry-settlement case's
  existing one-second negative timeout is a separately adopted exception,
  pending an upstream causal settlement observer; do not replace it with an
  arbitrary yield or a weaker polling assertion.

Confirm all jobs, fixture children and runtime owners settled. Keep raw logs,
commands, source state, per-case results and readbacks before removing owned
scratch. Shared Cargo cleanup follows the repository policy after every
consumer releases the target; a successful observation does not itself require
cleaning another agent's build output.

## Checked SQL and retained upstream ownership

The fixture's `consumer-checked-queries.json` is verified by the package build
when `test-support` is enabled. The additional integration case
`consumer_checked_queries::consumer_checked_queries_match_the_native_schema`
calls the pinned generator's native `generate_manifest(..., true)` against real
Turso. A current generated file alone does not replace either gate. Keep stale
input rejection and exact restoration as separate sensitivity evidence only
when the assignment authorizes the mutation.

The native `engine_keys` audit here retains the fleet schemas, NOT NULL key
metadata, NULL refusal and valid-insert controls. Its journal slice stays with
agent-fleet-sync. The two raw workerd `journal/*` migration cases also stay
there. A fleet pass does not authorize removal before those upstream targets
are concretely preserved.

The relocated aggregates replace upstream standalone consumer commands and
legacy shell journey paths. General durable-steps/query-generator/DO scope
recipes remain with their owning repository. Return each unsupported or
unavailable control explicitly; native evidence establishes no production
Router replacement, hosted Cloudflare, provider, release or merge acceptance.
