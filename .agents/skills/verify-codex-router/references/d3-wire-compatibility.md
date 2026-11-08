# D3 local wire compatibility

**Draft until its declared real-target pass.** Run only when commissioned for
central-execution-model D3. This journey uses two signed debug builds of the
exact pre-D3 0.1.134 source at `247570818` against the declared branch's local Tokio central.
It launches no production Router, uses no SSH or Keychain, and sends no model
input. Label evidence **pre-D3 0.1.134 source, debug build**. It does not prove
the release-profile artifact or hosted staging.

Oracle: central-execution-model `plan.html#d3`, `verification.html#j11/#j22`,
with the Lead's D3 target/scoping corrections: local compatibility, actual
100 retained notices and 50 queued messages with nothing refused or lost.
The ≤32 unanswered window, absence of notice resends/wakes during one real hour,
and conformance alarm-wake-by-cause observation are **observed from D4/D5**.
They remain obligations; D3 does not execute or claim them.

The retired 0.1.125 target is incompatible with the owner-approved D1 hard
cutover; it has no ProjectListChanged or Push/Pushed vocabulary. The Lead's
2026-11-08 disposition corrected J11/J22 to the release before D3. Preserve old
failed observations as retired-target evidence; do not add a legacy shim.

## Bind the sources and signed artifacts

The maintained entry is:

```sh
node verification/run.ts --help
node verification/run.ts --d3-wire "$D3_PLAN"
```

`verification/observations/central/d3-wire-compatibility/compatibility-plan.ts`
owns the strict owner-private JSON declaration. It names the repository and full
stacked `sourceHead`, current `apparatusHead`, and frozen `productHead` used for the central build,
exact old source commit `247570818a922535043bd935f3eb3d93f3ec5a67`, old source directory and Git archive
path/hash, old Router/CLI, native executable and central driver artifacts
(path/hash/identifier/team), fresh evidence directory under repository `tmp/`,
and UTC deadline. Source/build equivalence and the build/signing receipt belong
in that evidence. The driver verifies the archive's embedded commit; the caller
must also preserve its clean source/build custody. A label alone cannot prove
the executable was built from it.

Use the assigned `codex-router.feat-fleet-link-health` worktree at `247570818`.
Keep its preexisting dirty files untouched; commit nothing there. Never
cargo-install it. Use its normal `target/debug`, its signing runner, and one package per compiling
window. Coordinate the slot with the Implementer and stop below the commissioned
disk floor. Build its `codex-router-cli` and `agent-collaboration` packages
separately; the current branch supplies
`codex-router-tailnet-verification-host`. Sign every exact direct-launch Rust
executable with its project's runner before hashing it for the declaration:

```sh
scripts/cargo_debug_signing_runner.sh --sign-only "$EXECUTABLE"
codesign --verify --strict "$EXECUTABLE"
codesign -dv --verbose=4 "$EXECUTABLE"
```

Require TeamIdentifier `974QD84WVC` for our old Router/CLI and central helper.
The driver checks each declared identity/team/hash immediately before launch.
Native bytes must be a separately declared, signed task-owned artifact; copy and
sign that artifact before hashing, without changing the owner's installation.
Keep signatures/build logs outside `target/`. No unsigned or Keychain fallback.

## Real path and readiness

The controller reuses the helper's existing `d1-central` action solely to start
the real Tokio central and fixture Access issuer. It never uses the current
source `d1-host` action as the old side. Actual old-source Router foreground Hosts
own their children, with fresh private roots, Codex homes, dedicated short native
socket paths and free nonreserved loopback ports under the recorded task
allocation. These fixture homes are separately authorized for this journey;
ordinary normal-home guards remain unchanged.

The old debug CLI's actual `fleet enroll --allow-plaintext-file-secrets` initializes
its isolated plaintext roots. The fixture Access edge translates only synthetic
auth; the real central admits the machine and the SessionGate forwards real
WebSocket bytes. The old Host then connects normally. Native initialize and
metadata-only thread creation provide an actual recipient address. Before
unloading it, call the real `thread/archive`, then `thread/unarchive` in that
fixture's isolated Codex home. Codex 0.166.0's archive implementation explicitly
persists a fresh loaded non-ephemeral thread before archiving; unarchive restores
that empty persisted thread without inference. Afterward require its absence
from `thread/loaded/list` and a successful `thread/read` with the exact id,
`status.type: notLoaded`, non-ephemeral state and path inside that fixture's
sessions directory. Create no rollout by hand. Stop if any condition fails.

This route is version-bound to the actual native artifact and its schema.
Source: `openai/codex` `rust-v0.166.0`, commit
`6ff1e1ca3b30fc8b914720681fcf5fe62e95f856`,
`codex-rs/app-server/src/request_processors/thread_processor.rs`,
`thread_archive_response` and `thread_unarchive_response`. Qualify a new native
version before reuse. The fixture is an empty persisted recipient, with zero
model turns; it proves central↔Router wire and inbox custody, not inference or
model-created conversation persistence. Keep the actual method replies and
turn-start count. A guard's blocked startup connection is not a model response.

Require pre-D3 source version 0.1.134 health, service publication, actual native
endpoint readiness, distinct installation identities and observed Welcome on
each central edge. No paid prompt is sent. Queue messages must reach actual held
inbox records with matching sender status; model delivery remains outside scope.
An unexpected turn fails this no-input run.

## Drives and deciding evidence

- **J11:** actual public board writes and exact replicated marker readback on
  the other Host, then distinct queue-mail items both directions with sender and
  recipient readbacks. Reject capability-bearing old-client Welcomes, unknown
  frames, refusals and missing Welcome. Keep observed operation kinds. Untriggered
  wire variants remain explicit coverage gaps; a pair smoke is not exhaustive
  proof of every possible operation.
- **D3 J22:** stop B, make 100 genuine public mutations producing retained
  notices and admit 50 distinct queued messages. Reopen the same old-source B.
  Require real notice-page catch-up, exactly one single Push per expected mail
  id and matching actual inbox custody for all 50. No sender receipt alone earns
  arrival credit. Catch-up is triggered by Welcome; do not inject pages or
  fabricate notice counts. A missing retained page is a failure, not a skipped
  assertion.

`SessionAudit` observes complete original server and masked client payloads
without changing forwarding. Every deciding boundary checks valid raw audits
for both Hosts, including after cleanup. The evaluator counts literal notice
positions and Push identities, and correlates answers to exact outgoing request
bodies by frame UUID. InvalidShape is a refusal, never ignored. Retain original
raw failures. Focused tests reject actual encoded unknown frames (including
A-only corruption), unknown/new fields, duplicate or missing backlog items and
incomplete fragmented payloads. Those tests qualify
the apparatus and never substitute for the actual two-Host drive.

## Evidence and cleanup

Keep declaration/archive hash, actual build/signing receipts, allocation and
readiness, public calls/readbacks, complete process output, per-journey JSON and
final `result.json`. A failure keeps its original evidence. Check source and
artifact hashes after observation; a new product build needs a new declaration.

Close native observers, exact recorded Hosts/children, SessionGate/Access edges
and the owned central independently, with fresh process identity checks. Retain
state/evidence for inspection; directory deletion is not part of this driver.
No normal home, production process or login Keychain is read or changed by the
journey. A setup failure or incomplete cleanup is not a compatibility pass.
