# Normal Worker session and holder proof

Base: `cb930a013d338d7ab2efa702f7dcf5a9407838d0`, branch
`feat/central-one-core`, library `68394e062e143a133d46943ef23d01095ab5fe7b`.
This is B's local component for the seven session and five holder groups retained
by decisions 508/513. Lead decisions 604, 616 and 620 govern the replacement.
The canonical plan remains r4, terminal `pr-ready-unmerged`; this document does
not declare whole PR3 ready or accept the component on the Lead's behalf.

Initial real runs pass 2 capability, 5 session-failure and 5 holder groups, all
exit 0. Thirteen ordinary incorrect builds compile and are rejected by the
permanent scenarios, all runtime exit 1. No historical pre-fix behavior red was
observed. Final narrow quality, production exclusion and restored runs pass.
The integrated full gates pass at frozen edcadfba; the proof-only receipt
commit is the next immutable declaration. Evidence: `tmp/worker-session-proof/`.

## Real paths and deciding observations

The existing `session_scope_workerd` example starts one owned Miniflare/workerd
process. Every scenario enrolls actual installations and authenticates normal
`/v1/session` sockets, then uses the existing shared session rules, Registry
publication, core store boundary and actual Cloudflare socket capabilities.
No legacy shell, injected registration, fabricated peer directory or grant is
used. Held schema publication is the existing runtime dispatch control at the
actual Connection object. Fixture Access/JWKS remains local setup, not hosted
Access proof. The fixture helper now permits pausing after real authenticated
upgrade and before Hello; the action still sends the real wire Hello.

| Retained group | Replacement oracle | Incorrect behavior caught |
|---|---|---|
| workers-rs codec and original origin | The actual normal socket roundtrips `(0,1,0)`, `(9007199254740993,9007199254740993,i64::MIN)` and `(u64::MAX,u64::MAX,i64::MAX)` for support/attach sequence/expiry, with its Unicode admission origin unchanged; the original attachment is restored and Hello replays exact Welcome. | Rounding the actual integer serializer. |
| same alarm owner and competing deadlines | Read the alarm from the same actual State containing the admitted connection. Two real pending holder calls have deadlines `1900000020000` and `1900000025000`; the alarm retains the first. Both answers complete their exact caller journals. | Shifting the actual scheduled deadline. The former SqlTx pointer identity is a dead contract under 604. |
| committed attachment error | One selected actual socket fails Attached publication after the real commit and physically closes; the fault records that exact connection. | Omitting the failed-socket close. |
| healthy socket usable | A separate normally enrolled installation retains its exact Attached registration and repeated same-session Welcome on the original socket. Separate installations are explicitly accepted in 616. | Dropping replayed Hello on Attached state. |
| Register writer mismatch | An unequal writer receives exact correlated NotWriter; the same socket immediately replays Welcome. | Flattening the correlated refusal to InvalidShape. |
| callback rollback | A one-shot callback error follows measured real writes when Welcome/Attached was staged. Read-only public journal inspection finds the real report at 0 and no attach receipt at 1; registration stays Attaching, no Welcome precedes an ordered socket barrier, and the real retry succeeds. | Committing callback writes before returning the error leaves the attach receipt. |
| SQL rollback | The same sequence executes a real SQLx integer-overflow error, retains its Backend cause, observes no attach receipt/Welcome/Attached leak, and retries successfully. | Committing before returning that SQL error leaves the attach receipt. |
| holder readiness/expiry/replacement | Opening, held Attaching and expired normal sockets have NeverSent caller journals, no waiter and no frame before their ordered barriers. Same-session replacement physically closes the old connection and the new connection answers a real Release. | Selecting an expired binding emits an actual Release before refusal. |
| exact connection/frame and separate event | Another normal connection and a wrong frame are causally observed as validated answers without consuming the target waiter. Exact response completes the real Registry call from a separate socket event. | Ignoring frame identity consumes the waiter. |
| close/alarm/late answer | Actual connection replacement closes a pending waiter; an alarm expires the next attempt. Retries retain CallId with new frame IDs; the obsolete frame cannot settle the fresh waiter. | Leaving the waiter past its deadline. |
| answer beside schema publication | A real holder call completes while another normal Connection object's schema report remains held; releasing that report then produces its exact Welcome. | Waiting for a network answer inside the local exclusion section. The same permanent case can be selected alone as `holder-schema-overlap`. |
| SessionId dimension | After actual normal admission, the existing fresh HolderReplyHub primitive rejects changed SessionKey with the same connection/frame, then accepts the original key. In the same case, a changed Hello SessionId on the admitted wire receives typed MalformedFrame followed by physical close. | Ignoring SessionId in the hub; separately dropping the changed-SessionId wire frame. This is a primitive plus wire observation, never a wire-supplied isolated hub key. |

## Fault and observation ownership

`SessionTransaction` receives only the owned `SessionFinalizeFault` enum. It
runs the real library callback before the selected error, only when that callback
staged Welcome or Attached. The existing StepCallback remains private and no
platform handle enters it. The checked conformance-only SQL measures actual
`total_changes()` and executes `abs(i64::MIN)` through SQLx. Generation and
metadata are producer-owned; no private library query was copied.

The typed marker retains the actual outer Aborted/Backend class and the SQLx
Backend cause. The runtime records the one-shot observation after the real
store returned the rollback error, outside the transaction. It does not become
a terminal operation refusal or replace durable retry. The callback and SQL
commit-before-error variants both leave an actual Local attach receipt at 1,
which the real journal assertion rejects.

The selected publication error is at CloudflareStorageHost's actual attachment
capability. The codec and alarm readbacks use the normally admitted socket and
State. The one-shot barrier and journal reader live at
`conformance_scope_faults`, under `conformance-faults` per 620. Arming requires a
live actual normal connection; the barrier consumes that connection's arm and
sends one binary frame without changing protocol or registration. Public
JournalStorage operation/step methods read the held report's real CallId.
Generic RuntimeControl and Native fault behavior are unchanged. New private
routes require the existing owner authentication and are absent from production.
The growing object-shell conformance handlers moved into a child module at the
same owner; the main shell stays below600 lines.

## Proof accounting and limits

The authoritative incorrect-build list is `incorrect-build-summary.json`:
13 successful builds, 13 rejected runtime runs, with exact source restorations.
The first expired-holder check only timed out in a synchronous HTTP call, so it
was replaced by concurrent direct wire observation. One earlier frame-control
receipt overlapped that fixture edit and is also excluded; the final frozen
source rerun is counted. The permanent scenarios' expectations were not weakened.
All eight temporarily mutated production owners match cb930a01 again.

The actual SQL error emits a caught workers-rs JS diagnostic about integer
overflow. The run continues to its typed Backend observation, rollback readback
and successful real retry. Existing Cargo diagnostics about unrendered
workers-rs template manifests also appear before successful commands; that
upstream checkout is untouched. Setup compile/CLI errors are retained and never
counted as product behavior reds.

Raw `serde_json::Value` is retained only at fixture wire/HTTP observation
boundaries to compare independent literal envelopes and send deliberate invalid
frames. Fault choices, one-shot states, errors, normal bindings, holder calls and
answers use their existing types. This adds no persisted schema or data-format
change, dependency, normal protocol mode, staging action or production process
replacement. Rust and Python are the changed language standards.

Worker eviction/hibernation, hosted/staging behavior and whole PR3 remain open.
Native process recovery belongs to lane F's separate accepted 618 component;
its five paths are imported unchanged as edcadfba, and the integrated full
native/workspace gates pass below. The paired Verifier's observation of this component
is also pending. Formal implementation review and final A pin/restack remain
outside this bounded local proof.

## Final narrow receipts

All commands ran from the B root with its own target, debug/test symbols 0,
incremental off, jobs 4, offline Cargo and clang. Each build's actual df check
remained above 45 GiB. Qualified Node 26.11.1, Miniflare 5.20261012.0-beta,
workerd 2026-10-12 and worker-build0.8.7 match the retained tool hashes.

- session-query-freshness-final: pinned generator native --check, exit 0.
- session-core-quality-final: core/conformance/Tokio library Clippy with
  conformance and -D warnings, exit 0.
- session-worker-quality-final: CF wasm library Clippy with conformance-faults
  and -D warnings, exit 0.
- session-example-quality-final: permanent example Clippy, exit 0.
- session-normal-native-check-final: core/Tokio normal no-default-features
  library check, exit 0.
- bundle-guard-python-quality and bundle-guard-python-format: Ruff check and
  format check of the changed structural gate, exit 0.
- session-production-worker-final: documented worker-release/no-default-features
  build, exit 0. Production guard exit 0; a separate byte scan finds all 15
  selected markers absent. Exact release files are retained in production/
  with hashes in production-marker-absence.json.
- session-restored-worker-final: conformance-faults dev rebuild, exit 0. The
  converse faults bundle guard also exits0.
- restored-session-capabilities:2/0/0, restored-session-failures:5/0/0,
  restored-holder-lifecycle:5/0/0; each exit 0, each owned process reaped.
  The selected holder-schema-overlap is the same retained group, not a sixth
  holder scenario or a second counted success.
- Full workspace formatting and git diff whitespace checks exit 0. Nine final
  compiler/runtime source maps match the final source bytes. The owned18093
  listener is absent after the final runtime. final-source-freshness.json
  retains that check.

The Lead granted the four full gates at frozen edcadfba in 01a1591c-0abb.
START receipt 01a1591c-93f8; END/returned slot01a15922-acd5, received by Lead
in 01a15922-e1fb. All four gates exited0 in their exact authorized order:

1. session-full-native:275 passed,0 failed,1 ignored across 68 targets,61
   nonempty. process_restart is3/0/1; its ignored child launcher is apparatus,
   not a fourth scenario or duplicate child count. full-native-counts.json
   retains the per-target accounting.
2. session-full-quality: all-target strict Clippy for core/Tokio/conformance,0.
3. session-workspace-default: workspace all-target check,0.
4. session-workspace-conformance: same with the declared conformance features,0.

All four source maps match current code. The tree stayed clean at edcadfba
through the gates. F's accepted 618 commit6a462358 was cherry-picked from a clean
B point after e6c87a59; all five paths were checked byte-identical. No F worktree
was touched. The imported native process tests passed in the integrated suite.

Cleanup followed the project policy after every own job ended and lsof +D on
this exact target returned no consumers or diagnostics. The dry-run preview
selected 8324 files/9.6 GiB; actual workspace dev clean removed 8318/9.4 GiB, exit 0.
Actual df free space went 230 GiB → 239 GiB. target/debug went 18G → 8.0G; dependency
artifacts remain as the project clean preserves them. Raw receipts, all failure
logs/patches, counterexample states and exact release/dev bundles remain outside
the cleaned output. No further compiler/runtime work is running.

This own component is ready for paired observation, not accepted on the Lead's
behalf. Final A pin/restack, whole-PR review/readiness and hosted/staging gaps
remain open. The final receipt commit changes proof documents only; the code
and tests are exactly the validated edcadfba source.
