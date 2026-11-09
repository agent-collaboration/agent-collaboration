# Fleet copy and migration SQL on workerd

Run all **31 Rust adapter controls plus seven raw-SQL controls** on the
repository's pinned local Miniflare/workerd runtime. The adapter is an excluded
standalone Cargo workspace at `crates/fleet-project-copy/tests/workerd` with
its own lock and target. Native Turso is the fixture producer, not a substitute
for workerd SQLite. This route has no hosted Cloudflare or production scope.

The adopted source is agent-fleet-sync b9470dc's
`tests/normal-migrations-worker` and the fleet part of
`tests/integration/public-migration-sql.mjs`. The current exact identity
contract is `verification/fixtures/fleet-copy-workerd/case-identities.json`.
Preserve optional supplied snapshots outside the fixed 38-case denominator.

## Run the existing launcher

Declare an immutable head, native signing configuration, installed Node/pnpm,
wasm target, a task-owned writable copy of wasm-bindgen CLI **0.2.129**, fresh
task evidence and the exclusive
build/runtime slot. Keep normal native `target/debug`; the standalone worker
uses its own target, preventing native fixture features from joining its wasm
closure. Use the existing verification lock, not a second runtime installation.
Before launch, place a **regular writable copy**, not a symlink to an installed
location, at `$TASK_TMP/wasm-bindgen-0.2.129/wasm-bindgen`. Keep this dedicated
directory task-owned and declare it in the caller's write scope. On macOS the
launcher invokes the signing runner for the tool: **the runner signs every
executable in that containing directory**, not just wasm-bindgen, and writes
`.debug-signing-verified` plus temporary sweep/signing metadata there. This is
why the copy belongs in its own directory instead of a shared tool installation.

From the repository root, with `TASK_TMP` set to the declared absolute task-owned
temporary directory and the qualified CLI copy already in place:

```sh
: "${TASK_TMP:?set the declared absolute task-owned temporary directory}"
pnpm --dir verification install --frozen-lockfile
node verification/tooling/run-fleet-workerd.ts --help
df -h /System/Volumes/Data
WASM_BINDGEN="$TASK_TMP/wasm-bindgen-0.2.129/wasm-bindgen" env -u GITHUB_ACTIONS CARGO_BUILD_JOBS=2 node verification/tooling/run-fleet-workerd.ts "$TASK_TMP/fleet-workerd-evidence"
```

The tool-copy directory and fresh evidence location must both be declared. Help must
exit before tool lookup, evidence creation or builds. The launcher refuses
nonempty evidence and checks the 30 GiB disk floor before Cargo stages. It:

1. Executes signed native e2e rename/certainty exports, receiver FK library
   exports and the integration writer-order export against real owning APIs.
2. Checks the isolated worker feature tree, builds its wasm, signs/runs the
   supplied bindgen CLI through the repository runner and verifies its version.
3. Generates a lossless checkpoint manifest, validates all five generated
   runtime modules, then launches the actual Rust DO adapter and raw-SQL
   controllers. Each runtime waits for readiness and is disposed in `finally`.
4. Requires exact names and passing kinds. Only then writes `complete.json`.

Inspect `stages.json` for every actual command, exit and elapsed duration,
`matrix.json` for five target-produced streams/eighteen checkpoints, and each
controller's `results.json`. An empty, missing, duplicate, wrong-name or
`prefixOnly` result fails. Renamed/reordered sets are not interchangeable;
only reordering the same expected names is allowed. Optional snapshots require
one `snapshotPassed` per supplied entry; a run without them leaves that branch
unobserved.

## Fixed case contract

Every name below must appear once with `kind: passed` in its group's results.
The JSON identity contract is the runnable authority; compare names as sets,
not just totals.

| Result group                    | Required names                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `rename` (5)                    | `warm_rename`, `reopened_rename`, `fresh_rename`, `fresh_chain`, `dropped_local`                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `certainty` (4)                 | `rollbackAfterApply`, `lostRollbackReply`, `lostCommitReply`, `unknownRollbackCode`                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `foreignKeys` seed/order (2)    | `child-page-before-parent`, `orphan-seed-rollback`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `foreignKeys` affinity (14)     | `affinity-seed-text-integer-valid`, `affinity-apply-text-integer-valid`, `affinity-seed-text-integer-orphan`, `affinity-apply-text-integer-orphan`, `affinity-seed-numeric-text-valid`, `affinity-apply-numeric-text-valid`, `affinity-seed-numeric-text-orphan`, `affinity-apply-numeric-text-orphan`, `affinity-seed-blob-integer-orphan`, `affinity-apply-blob-integer-orphan`, `affinity-seed-blob-integer-valid`, `affinity-apply-blob-integer-valid`, `affinity-seed-parent-collation-valid`, `affinity-apply-parent-collation-valid` |
| `foreignKeys` action/split (4)  | `apply-NO-ACTION-false`, `apply-NO-ACTION-true`, `apply-RESTRICT-false`, `apply-RESTRICT-true`                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `foreignKeys` composite/CDC (2) | `nullable-composite-and-self-reference`, `real-writer-captured-order`                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `migrationSql` (7)              | `delivery/fresh`, `delivery/v0.4.0`, `delivery/typed-intermediate`, `lease/fresh`, `lease/v0.4.0`, `session/fresh`, `session/v0.4.0`                                                                                                                                                                                                                                                                                                                                                                                                        |

Rename replay consumes original raw Piece strings; parsing them as ordinary JS
numbers can round wide integers. Require typed checkpoint rows, column layouts,
schema and acknowledged position, including local-table absence. Native reopen
names do not claim a workerd-process restart. The native bootstrap case remains
a native obligation and exports no additional fixed workerd case.

Certainty controls inject boundary reply loss after real commit/rollback and
inspect row/receipt counts. They must distinguish no-effect rollback from
committed answer loss, preserve typed reasons, reconcile the identical Piece
once and reject a second effect. This proves local classifier/storage behavior,
not physical I/O failure or session containment.

FK controls inspect exact integrity refusals and rows/receipts/copy state after
rollback, affinity/collation rules, split apply, nullable composite/self keys
and actual writer CDC order. Raw-SQL controls read this repository's production
migrations/queries and retained v0.4.0 seed. Require all migration files, no
scratch `_uuid_` columns, namespace isolation, exact preserved timestamps and
typed custody/status projections. The two `journal/*` SQL cases and native
engine-key journal slice remain upstream obligations.

## Compatibility, measurements and cleanup

`prepare-matrix.ts` replaces the original Python oracle and must retain wide
integer lexemes, blob/NULL values, final split-part position matching, offline
streams, absent-table checks and optional snapshots. Keep its permanent
sensitivity tests and literal oracles; a count-only manifest is insufficient.

The existing verification runtime pins and Miniflare converter are the adopted
compatibility path. `worker-modules.ts` declares five actual generated runtime
assets, shim first, and recursively matches every `.js`/`.mjs`/`.wasm` file;
extra/missing files or changed snippet hash fail before launch. Only declaration
metadata is excluded. A newer-runtime-only failure needs its own resolution;
preserving names does not justify changing the case assertions.

The existing Central Worker CI job owns these steps and its existing controls,
with the 30-minute cap retained. Record complete GitHub job start/end and tool
cache state separately for cold and warm measurements. Local launcher duration
or its proof-phase timing receipt cannot establish whole-job duration. A missing
cold or warm measurement stays unverified; preserve every case when reporting
an over-budget run.

Keep raw stages, result JSON, exports, source/configuration fingerprints and
wasm/tool hashes outside build output. Confirm owned native children and DO
runtimes stopped before cleanup. Do not remove evidence or shared build output
while another observer uses it. Return native export counts separately from
31 adapter + seven SQL outcomes, supplied snapshot coverage, timing limits and
any unavailable controller/runtime boundary.
