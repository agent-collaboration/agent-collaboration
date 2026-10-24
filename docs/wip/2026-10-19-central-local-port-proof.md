# LocalServer scenario port: implementation checkpoint

Status: bounded implementation handback in progress; full cutover remains incomplete.
This record maps every original scenario, including the exact work still owed
behind product dependencies. The port base is
`0185d0c33e7973a7052004c06e459ea709b420d9`.

The approved outcome is one-core Program Design sections 5, 13 and 14, and plan
rows B1b and B6. The original LocalServer tests remain untouched.

## Placements and current proof limits

Decision 472 assigns cases requiring owner import HTTP to D1b, above this base.
Decision 473 assigns the operations view, move/takeover and retire admin routes
to B's owed product work. Those cases remain untouched under (d) and (e).
Import used only as fixture setup does not by itself move a case to D.

Decision 474 assigns the reproduced staged-part expiry defect to B. The
permanent regression is the test-only commit `66f574a2`; it invokes the real
Copy alarm after owning Turso reopen and a two-day Clock advance. One actual
staged row remains where zero is required. This is (f), not port credit. It
proves the missing cleanup before the broader automatic-wake journey can pass.

The Lead correction to decision 475 retains UUIDv7 TEXT primary keys and moves
old integer keys to `key_value`. Both `key_value` and `record_value` are now
untyped in both scalar tables, preserving every submitted INTEGER/REAL tag.
The original typed-affinity checkpoint masked input-tag changes; its passing
values alone did not establish that sensitivity.

## Inventory and disposition progress

All original paths below are relative to
`crates/collaboration-central-conformance/tests/`. Shared-body paths are relative
to that crate's `src/`. Sub-scenario retirements are explicit below. A prerequisite regression or
source dependency never counts as the full scenario passing.

| # | Original file and test | Requirement and real boundary | Disposition / evidence | Run |
| --- | --- | --- | --- | --- |
| 1 | `local_archive.rs` / `automated_archive_frames_preserve_first_report_and_exact_no_ledger_finalization` | Authenticated archive reports retain the first report, reject invalid reports, finalize exact board/workspace records and preserve the request ledger; real sockets and Turso. | (f) Native port passes in `crates/collaboration-central-tokio/tests/archive_reports.rs`; shared `archive_runtime.rs` preserves the complete oracle through its supplied-clock callback. Workerd waits on B decision 477 compound SELECT fix; unchanged archive mode is the integration acceptance. | Native 1 passed, exit 0; workerd failed at real FleetSnapshot SQL, exit 1; no cross-runtime port credit. |
| 2 | `local_archive.rs` / `physical_native_registry_restart_gates_publication_and_fences_old_grants` | Physical restart retains registry publication order and refuses old grants. | (f) B product dependency: `registry_lease_calls.rs:62` accepts only Renew at this base; ordinary claim/first epoch and Directory are prerequisites. Regression `a9da23da` retains the creation and Directory failures. The full original publication/restart oracle remains owed. | Prerequisite regression red, exit 101; full scenario not run. |
| 3 | `local_archive.rs` / `physical_native_restart_retries_release_and_acknowledgement_loss_from_persisted_alarm` | Both lost dispatch and lost acknowledgement recover from a persisted archive-cleanup alarm after physical restart. | (f) B product dependency: the original `archive_cleanup_restart.rs:44` uses an ordinary Claim and real reservation/fence refusal before cleanup. The composed Registry accepts only Renew at this base. Both cleanup loss/restart oracles remain owed; the old import fixture is incidental. | Full scenario not run; no cleanup credit. |
| 4 | `local_conformance.rs` / `shared_copy_suite_on_real_turso` | Ordered apply, permanent receipt replay, staged assembly, fencing, isolation, exact reads and rollback on real Turso. | (b) `composed_copy_suite.rs` plus `composed_copy_fixture.rs`, native `tests/copy_suite.rs` and workerd `composed-copy-suite`. All surviving original assertions run through real sessions/storage; seven actor evictions become whole-runtime stop/restart callbacks. Three legacy corruption probes and pure Return guards have explicit retired sub-dispositions below. | Native 1 passed, exit 0; shared workerd suite passed, exit 0; sensitivity/restoration recorded below. |
| 5 | `local_conformance.rs` / `shared_session_suite_on_real_websockets_and_turso` | Session registration, lease operations, replacement and transfer cases through real sockets and storage. | (e) B admin routes: `session_scenarios.rs:56` calls owner takeover preview; `:376` constructs owner Move. Keep the full shared suite untouched until those routes land. | Not run. |
| 6 | `local_conformance.rs` / `session_recovers_lost_ack_after_entire_local_server_is_reopened` | Piece and renewal answers are lost; physical server reopen recovers the original acknowledgement, lease and unchanged renewal receipt. | (f) B ordinary register/first-epoch dependency, decision 491. `session_resume.rs` retains the original expiry, piece replay and two renewal replays; controls now exist, but no partial receipt test substitutes for that complete journey. | Prerequisite regression a9da23da red, exit 101; complete row not run. |
| 7 | `local_conformance.rs` / `transaction_rollbacks_and_complete_concurrent_reads_survive_local_server_reopen` | Rollback, fence and complete concurrent snapshot prefixes survive physical server reopen. | (b) `transaction_reopen.rs` / `ComposedTransactionScenario`, native `tests/transaction_reopen.rs`, workerd `composed-transactions`: real late uniqueness and late callback Full rollback, immutable fence, matching new epoch, complete 300-row concurrent reads with 256/44 pages, then physical whole-runtime reopen. Observer keeps reading until the complete new prefix and rejects an old prefix after acknowledgement. | Native 1 passed, exit 0; workerd passed, exit 0; half-rows wrong build failed 0/1 exit 101. |
| 8 | `local_conformance.rs` / `lossless_sql_scalars_survive_real_storage_and_snapshot` | Exact large signed integers and REAL identity survive real storage and snapshot. | (b) `composed_numeric.rs`, native `tests/numeric_composed.rs`, workerd `composed-numeric`; actual scalar cells, INTEGER/REAL identity, receipt replay and whole-runtime reopen. UUIDv7 key adaptation follows corrected decision 475; both value columns in both tables are untyped. | Corrected originals pass native and workerd, exit 0; both input-tag mutants fail at the scalar-tag check on both runtimes (native 101, workerd 1). Details below. |
| 9 | `local_conformance.rs` / `staging_safe_session_groups_pass_on_real_websockets_and_turso` | Staging-safe session groups, held-renewal replacement and live revocation on local real sockets/storage. | (e) B admin routes: `staging_scenarios.rs:36` invokes `transfer_crash_and_preview`, which calls the owner preview/takeover route through `session_scenarios.rs:56`. | Not run. |
| 10 | `local_conformance.rs` / `staging_normal_rows_pass_across_local_server_reopen` | Normal move/forward, lost owner response, renewal replay and same-socket observations; server shutdown closes the original socket and reopened state recovers. | (e) B admin routes: `staging_transport_rows.rs:47` constructs owner Move; `staging_normal_rows.rs:118` calls takeover preview. Also uses imported fixture state. | Not run. |
| 11 | `local_conformance.rs` / `held_fence_and_held_piece_resolve_to_one_order` | Forced fence/piece overlap resolves to one complete ordered prefix. | (e) B admin routes: `transfer_scenarios.rs:284` constructs takeover from owner preview; `:292` runs that owner HTTP call concurrently with the held piece. | Not run. |
| 12 | `local_conformance.rs` / `surviving_normal_socket_is_read_before_the_honest_reconnect` | A socket that remains live is read before reconnecting; no reconnect can hide its failure. | (e) B admin routes: `prepare_staging_normal_rows` calls owner takeover preview in `staging_normal_rows.rs:118`, before the surviving-socket observation. | Not run. |
| 13 | `local_copy_operations.rs` / `owner_health_uses_target_measurement_before_first_migration_and_refuses_machine_access` | Owner HTTP Health works before migration, reports independently measured database/WAL bytes, ignores a forged size query and denies machine access. | (e) B operations view: owner Health URL at `local_copy_operations.rs:44`; preserve independent bytes, pre-migration metadata and machine denial. | Not run. |
| 14 | `local_copy_operations.rs` / `owner_health_remains_reachable_after_a_real_late_migration_failure` | A genuine late migration error remains typed, while owner HTTP Health reports recorded version, exact failure and real bytes. | (e) B operations view: owner Health request at `local_copy_operations.rs:131`; retain actual late migration fault and exact failure readback. | Not run. |
| 15 | `local_copy_operations.rs` / `owner_preview_reports_real_copy_migration_failure_as_503_then_recovers_after_restart` | Real offline migration-history fault yields exact owner preview 503; repair and physical restart recover preview 200. | (e) B FL-10 routes: owner `/admin/v1/projects/{id}/takeover/preview` at `local_copy_operations.rs:212`; retain 503, offline repair, restart and 200. | Not run. |
| 16 | `local_copy_retirement.rs` / `owner_retired_health_repair_import_and_migrate_use_the_minimal_receipt_across_restart` | Minimal retired receipt remains readable without migrating; owner Health, Repair, import and Migrate return their exact outcomes across restart. | (d) D1b required: direct owner project-import request at `local_copy_retirement.rs:97`. Also blocked on Health/Repair/Migrate; this mixed scenario is not ready for D merely because import is assigned there. | Not run. |
| 17 | `local_fleet_feed.rs` / `fleet_feed_survives_physical_restart_and_prunes_by_central_append_time` | Old source-time imports survive 29 days, expire by central append time; persisted alarms, exact feed positions, prune-all high-water readback and catch-up survive physical restarts. | (d) D1b required: `fleet_feed_restart.rs:119` performs owner Fleet import; line 137 checks preserved original times. Native `fleet_retention.rs:129` has no physical server restart or old imported source time and is not equivalent. | Not run. |
| 18 | `local_fleet_receipt_ids.rs` / `workspace_archive_receipts_are_not_limited_to_eight_shell_call_ids` | Archiving nine boards and their workspace returns all ten exact effect identities, with no truncation or next page. | (b) `crates/collaboration-central-tokio/tests/fleet_requests.rs:35` / `archiving_nine_empty_boards_returns_all_ten_exact_effect_identities`: actual machine frame, all ten exact references, count and absent continuation. | 1 passed, 4 filtered, exit 0; valid eight-entry wrong receipt failed 0/1 exit 101. |
| 19 | `local_forwarding.rs` / `forwarding_and_atomic_overload_race_use_real_clients_and_turso` | Real forwarded request/reply, refusal/deadline cases and atomic overload race through clients and Turso. | (f) B exhaustive routing dependency, decision 476. Actual admitted ForwardedPost frame resets its real socket at this base; `session_route_gaps.rs` regression a81ea84c. Full forwarding/deadline/overload race remains owed. | Permanent prerequisite red, exit 101; full row not run. |
| 20 | `local_import_claim.rs` / `fleet_conflict_preserves_exact_decision_records_and_feed` | Conflicting Fleet request preserves the original decision, canonical records and feed. | (f) `composed_fleet_conflict.rs` preserves exact original decision and all canonical records/head before and after replay/conflict. Native `tests/fleet_conflict.rs` passes; workerd needs B decision 477 canonical Snapshot SQL fix. | Native 1 passed, exit 0; workerd compound SELECT failed and closed the session, exit 1; no full port credit. |
| 21 | `local_import_claim.rs` / `ordinary_fleet_admission_and_first_epoch_run_through_real_session` | Ordinary Fleet admission creates the first lease through the real session, including its creation chain. | (f) B first-epoch creation chain: `tests/fleet_creation_gap.rs`, regression a9da23da, accepts Workspace/Board/Directory but first real schema piece returns Uninitialized. Metadata admission is not first-epoch proof. | 0 passed, 1 failed, exit 101; complete chain remains owed. |
| 22 | `local_import_claim.rs` / `fleet_import_owner_route_is_atomic_and_preserves_original_records` | Owner HTTP import preserves original identities, fields, times and provenance; exact replay/conflict/atomic refusal and machine denial. | (d) D1b: original call at `local_import_claim.rs:60`; shared `fleet_import_scenarios.rs:79` invokes `ConformanceClient::import_fleet`, whose HTTP route is at `conformance_client.rs:196`. | Not run. |
| 23 | `local_import_claim.rs` / `owner_import_publishes_an_unheld_readable_copy` | Owner HTTP project import publishes the exact imported head with no holder. | (d) D1b: `local_import_claim.rs:84` invokes `import_project`; product route at `conformance_client.rs:178`. | Not run. |
| 24 | `local_import_claim.rs` / `import_claim_race_and_validation_use_real_turso_and_websockets` | Owner import replay/digest/schema/order/full rollback, readable unheld copy, competing claims and first granted piece; real sockets and Turso. | (d) D1b: `import_scenarios.rs:10` uploads and line 147 replays the owner project import; `import_validation.rs` holds further import outcomes. | Not run. |
| 25 | `local_import_claim.rs` / `import_claim_and_directory_producer_resume_after_native_server_restart` | Project-import/claim and directory producers retain unfinished work and exact publication across physical server restart. | (d) D1b: `ImportClaimRestartScenario` in `import_restart.rs:15` and `ImportDirectoryRestartScenario` in `directory_restart.rs:31` require owner import preparation and resumed publication. | Not run. |
| 26 | `local_notices.rs` / `registry_notice_and_seven_day_alarm_survive_physical_native_restart` | Claim-generated notice, exact seven-day persisted deadline, reconnect readback and retention survive physical restart. | (f) B exhaustive Notices/Directory and ordinary Claim dependencies, decision 476; `session_route_gaps.rs` and a9da23da retain real-transport failures. Seven-day persisted alarm/restart oracle remains owed. | Prerequisite reds exit 101; full row not run. |
| 27 | `local_notices.rs` / `fleet_alarm_delivers_numbered_hints_through_real_native_sockets` | A real Fleet alarm delivers and persists board/feed hints with independent per-installation numbering. | (f) B exhaustive Notices routing dependency, decision 476. Actual valid Notices frame resets its admitted physical socket in regression a81ea84c. Per-installation hint numbering remains owed. | Prerequisite red exit 101; full row not run. |
| 28 | `local_notices.rs` / `fleet_outbox_restarts_after_dispatch_or_real_acknowledgement_loss` | Both lost dispatch and lost acknowledgement resume the stored Fleet outbox after physical restart. | (f) B notice/ordinary Claim dependencies, decision 476; full `fleet_outbox_restart.rs` dispatch-loss and acknowledgement-loss restart bodies remain owed. New controls alone are not proof of the outbox interaction. | Prerequisite reds exit 101; full row not run. |
| 29 | `local_session.rs` / `malformed_frames_get_correlated_refusals_or_typed_physical_close` | Four raw malformed-frame cases produce one correlated refusal or typed plus physical close; committed copy state remains unchanged. | (a) Covered by `crates/collaboration-central-tokio/tests/session_carryover.rs:38` and its `support/malformed_frame_scenarios.rs:23`: all four malformed classes, exact correlation, actual stored copy baseline and typed plus physical close. It additionally reads an independently healthy socket. Wire spelling follows the adopted library protocol replacing the deleted legacy protocol (one-core PD section 14). | 2-test session target: 2 passed, 0 failed, exit 0. |
| 30 | `local_session.rs` / `authenticated_upgrade_welcome_and_replacement_run_through_real_websockets` | Owner denied upgrade; machine authenticates, receives exact Welcome and replaces the same session's old socket. | (a) Covered by `crates/collaboration-central-tokio/tests/session_carryover.rs:32` and shared `session_replacement_scenarios.rs:11`: owner denied upgrade, exact Welcome/session identity, typed replacement, plus physical close and stable enrollment peer. The entry is the real authenticated WebSocket/Turso server. | 2-test session target: 2 passed, 0 failed, exit 0. |
| 31 | `local_storage.rs` / `legacy_actor_refuses_a_mailbox_without_creating_a_second_store` | Retained legacy actor cannot create a second mailbox store. | (c) Retired by one-core PD section 14: the whole `collaboration-central-local` crate and its independent per-object store are deleted. This case refuses only the legacy actor creating a second mailbox store; it does not test the new mailbox lifecycle. | Not run. |
| 32 | `local_storage.rs` / `real_turso_actor_reopens_ack_and_explicitly_rolls_back_late_constraint_error` | Late uniqueness failure rolls back both earlier rows and ack; eviction/reopen keeps the original ack and empty snapshot on real Turso. | (b) `crates/collaboration-central-tokio/tests/copy_rollback_reopen.rs`: real late UNIQUE error, typed failure, exact unchanged acknowledgement and empty snapshot before and after owning Turso close/reopen. UUIDv7 TEXT keys replace incidental integer fixture keys. | 1 passed, exit 0; injected leaked row failed 0/1 exit 101 and restored byte-exact. |
| 33 | `local_storage.rs` / `platform_alarm_survives_server_restart_and_cleans_staged_parts` | Part expiry survives physical server restart; overdue cleanup makes a repeated first part fresh with a new future deadline. | (f) Product defect, regression committed, waits on B: `crates/collaboration-central-tokio/tests/copy_staged_expiry.rs:22`, test-only commit `66f574a2`. Real stored staged row survives owning reopen and remains after a two-day Clock advance and real Copy alarm. Full automatic wake/server-restart port remains owed. | 1 failed, 0 passed, exit 101; intended row-count failure. |

## Execution evidence in progress

- Existing session coverage: `cargo test -p collaboration-central-tokio --features conformance --test session_carryover -j4 -- --nocapture`: 2 passed, 0 failed, exit 0.
- Staged expiry prerequisite: the same package/feature with `--test copy_staged_expiry`: 0 passed, 1 failed, exit 101. The intended assertion observed 1 expired staged row rather than 0, after both owning stores had closed. Rustfmt and its targeted Clippy pass. No fix or complete alarm port is claimed.
- Late-constraint port: `--test copy_rollback_reopen`: 1 passed, exit 0. It preserves the original acknowledgement and empty-row assertions before and after owning reopen. UUIDv7 TEXT row keys replace incidental fixture integer keys. The first compile exposed the library failure enum not implementing Error; it was converted at the test boundary before any scenario run.
- Late-constraint wrong build: a test-only fixture write deliberately leaked one row after the actual uniqueness failure. The unchanged committed snapshot assertion rejected it (0 passed, 1 failed, exit 101). This is post-green sensitivity, not a pre-fix product red. Product source was untouched. Restoration was byte-exact at SHA256 `0df3d1e0569d02441d90acdc9f259b7c312bca4e3f0265e4f1bda2ed91ed2d68`.
- Numeric native route: `--test numeric_composed`: 1 passed, exit 0. The shared body uses actual admitted machine frames, both scalar tables, exact snapshots, original receipt replay and whole native server reopen. Workerd whole-process reopen also passed, exit 0; sensitivity is recorded below.
- Nine-board archive receipt route: `--test fleet_requests archiving_nine_empty_boards_returns_all_ten_exact_effect_identities -- --exact --nocapture`: 1 passed, 4 filtered, exit 0. All ten original identities, total count and absent continuation are asserted through the real machine session. The valid eight-entry receipt wrong build failed; sensitivity is recorded below.

Current row-level dispositions: **2 covered (a), 5 ported (b), 1 retired (c),
6 assigned to D1b (d), 8 assigned to B admin work (e), and 11 dependent on B
product corrections (f)**. All 33 original rows are retained. These are
implementation dispositions, not Lead acceptance or independent observation.

## Decision 478 continuation

The commission now grants conformance-only wrappers in both runtime adapters
for CopyFull, DropNextAnswer, held dispatch and SetTime. The native manifest
may add only optional conformance and sqlx-core dependencies, enabled by the
existing conformance feature. Default dependency-graph equivalence and absence
of a normal dependency cycle are required. Core central_router and session_scope
remain outside the grant. The independent fault-dependent copy and transaction scenarios now pass.
Product-dependent scenarios retain their complete original proof obligations.

Current retained regression commits: `66f574a2` (staged expiry), `a81ea84c`
(Notices and ForwardedPost routing), `a9da23da` (Directory routing and ordinary
first-epoch creation). They remain red on this base; B owns their product fixes.
The new Directory case and creation-chain case each ran one test and failed
for their intended runtime reason, exit 101. B's later SQL snapshot fix is not
part of this base; unchanged workerd archive mode here exited 1 on its real
compound SELECT failure and reaped its owned process. Its later exact-time
check waits on this assignment's SetTime seam at integration.

The restored independent native targets passed 10 tests total across five
targets, exit 0. Package-scoped format checking passed, exit 0. All changed
native targets were checked by Clippy with warnings denied, exit 0. Numeric
workerd whole-process reopen passed and reaped both processes, exit 0.

Additional wrong builds, all post-green sensitivity with product source
untouched and unchanged observation assertions:

| Case | Injected wrong behavior | Observed result | Byte-exact restored source SHA256 |
| --- | --- | --- | --- |
| Numeric | Round submitted 2^53+1 INTEGER cells to 2^53 while keeping the literal expected cells. | Exact snapshot comparison failed; 0 passed, 1 failed, exit 101. | `fdffc7781c5f0c268799368167ec9fc4d4c7066ddc67e5e31457ae1d9fbbf205` |
| Archive receipts | The test transport adapter replaces the received ten-entry receipt by a valid eight-entry receipt before the scenario sees it. | Original completeness check failed; 0 passed, 1 failed, exit 101. | `ba0afca21299e1ab838f4f0d32802ddb0ab8b60cf93262903be9e82fc5ca6436` |
| Native archive reports | The supplied Clock capability skews the requested cut by one second. | Exact unchanged record/time comparison failed; 0 passed, 1 failed, exit 101. | `366aa85619c2306abf177e3156a255a15643448e5db76808180fe30ec495025d` |

The earlier five-target checkpoint had no remaining jobs. Task-owned tools
and build artifacts remain available for integration. Current final-job results
are recorded below; no independent observation is claimed.

## Decision 491: surviving copy suite and retired sub-scenarios

The copy suite's legacy raw-SQL controls are retired (c), per plan B2a line
265, which deletes central's own copy/fence rules. No library-private SQL was
added. The pure WireEffect Return cardinality/kind guard is likewise part of
the deleted legacy invocation mechanism (one-core PD section 14).

At pinned agent-fleet-sync revision `68394e062e143a133d46943ef23d01095ab5fe7b`:

- RemovePieceReceipts: underlying missing-receipt requirement is covered (a)
  by `crates/fleet-sync-receiver/src/turso_integrity_tests.rs:9`. It deletes
  real receipts, replays the old piece, requires Gap, and verifies unchanged
  rows/position. This is inspected permanent test coverage; no fresh library
  test execution is claimed here.
- InvalidateCopyFence: `turso_integrity_tests.rs:220` rejects corrupt stored
  copy records for both reads and apply. `turso_fence_integrity_tests.rs:314`
  through its shared helper at line 194 rejects inconsistent durable fence
  receipts and verifies unchanged durable state. These cover related integrity
  obligations; an exact invalid current-fence-epoch stored-record test has not
  been located and is a named library coverage gap, not a central port.
- ArchiveCopy: the legacy assertion actually acknowledges a new piece after
  setting the ignored archive flag (`copy_extended_scenarios.rs:74`). It is
  retired, with no equivalent claimed. Library retirement is a different
  state: `turso_rule_tests.rs:324` requires Retired for fence/status/apply/create;
  `turso_seed_tests.rs:454` reads a minimal retirement marker after dropping
  the other bookkeeping tables. These are related coverage, not equivalence.

## Controls and full transaction proof

Conformance-only control requests reach the actual owning composed object on
both runtimes. The shared `runtime_control_scenarios.rs` checks owner denial,
wrong-object denial, object-clock isolation through a real Fleet record,
actual late-write rollback plus full scalar readback, physical exact-frame
answer loss with independent same-frame traffic, durable replay, and held
original dispatch bytes with independent progress before resuming.

CopyFull measures real `total_changes()` within the actual AtomicStore
callback, then returns a StorageFull rollback error before commit once the
threshold is met. It is a **late callback failure**, not physical disk
exhaustion or interception of the exact Nth SQL statement. The normal store
owns rollback; no substitute database is used.

The current one-wrapper development workerd artifact passed controls, the
surviving copy suite and the tightened transaction scenario. Each owned
process was stopped and reaped; seven restart boundaries ran in the copy
suite. Three transaction verification markers are checkpoints of one scenario,
not three tests. Native controls and transaction tests each passed once.

Default native and Cloudflare wasm `cargo check --release` passed without
conformance. Conformance modules/fields/branches are feature gated. The native
default normal dependency graph is byte-identical before and after the exact
two optional manifest additions; conformance is absent by default. sqlx-core
was already transitive at the base. The conformance normal graph resolves
without a cycle. Targeted native Clippy (all nine affected targets) and workerd Clippy with
conformance and conformance-faults passed, exit 0, with warnings denied.
Native/conformance and Cloudflare format checks passed, exit 0. The pinned
workers-rs tooling also prints template-manifest diagnostics during the
Cloudflare Cargo invocation; the declared target proceeds and finishes
successfully. No upstream dependency was edited to hide those diagnostics.

Sensitivity: disabling the native store's actual late rollback branch makes
runtime_controls fail on CopyFull (0 passed, 1 failed, exit 101), and restores
byte-exact SHA256 `8f91db275037a87cc054a8db97b41d7a63dcdb05d8412ed86fd0f141f8250d8f`.
Submitting 150 rows with the unchanged 300-row oracle makes the tightened
transaction test fail (0 passed, 1 failed, exit 101); restored pre-format SHA256
`b129770db4116d8d7757ae67b8a9f77e6f79d93e211e479230fc24fd70645130`.
These are post-green wrong builds; no pre-fix product red is claimed for either.

The copy-suite wrong build replaced the submitted Delete with an Upsert while
keeping the exact expected snapshot. It failed after physical restart with the
extra second row (0 passed, 1 failed, exit 101). Source restored byte-exact at
SHA256 `c49b857fa5786504fe2631dd325af7088d6073feed70c3d1f770918f6823bc9e`.
This is input sensitivity, not a pre-fix product failure.

## Final bounded gate and remaining work

`cargo test -p collaboration-central-tokio --features conformance
--test runtime_controls --test transaction_reopen --test copy_suite
--test fleet_conflict --test session_carryover --test copy_rollback_reopen
--test numeric_composed --test fleet_requests --test archive_reports -j4
-- --nocapture` passed **14 tests across nine targets**, 0 failed, exit 0.
This gate deliberately separates the three retained product-regression targets;
the overall package suite is not claimed green.

Workerd modes `runtime-controls`, `composed-copy-suite`,
`composed-transactions` (tightened observer) and `composed-numeric` passed,
exit 0. Workerd `composed-fleet-conflict` failed on the real compound SELECT,
exit 1; its process stopped and reaped. The unchanged archive acceptance mode
still waits on integration with B's decision 477 fix. No staging, deployment,
production replacement, push, old-test deletion or independent Verifier
observation occurred.

All task-owned build/test/runtime commands are settled; all successful owned
workerd processes were explicitly stopped and reaped. Tools, persistence
fixtures and rebuildable artifacts remain for the Lead's integration and
Verifier observation. Free disk remained above the commissioned threshold.

The remaining work is not complete: D1b import surfaces, B admin surfaces,
B claim/first-epoch/Directory/Notices/ForwardedPost corrections, staged expiry,
and B's workerd Snapshot SQL correction must land before their full original
journeys can be ported or credited. The exact invalid current-fence-epoch
library stored-record test remains a named coverage gap for the Lead to route.

Proof limits: two earlier workerd negative-control HTTP attempts reset before
a response; their cause was not established. The rejection fixture now owns
a separate non-pooling HTTP client and the current run passed, but no
failure-rate improvement or root-cause fix is claimed. Workerd's expected
SQL-error cases print wasm-bindgen diagnostics while still returning the typed
failure and passing unchanged rollback observations.

Integration risk: conformance controls can change test-build fault selection,
clock scheduling and held-call timing. Release checks exclude them. The main
remaining risk is treating prerequisite evidence as completed recovery proof;
this ledger leaves those full journeys open. No merge-readiness or deletion
of the legacy implementation is declared.

## Row 8 correction after checkpoint 6523108e

Verifier B found that the REAL-affinity value column converted a submitted
Integer(1) into the expected Real(1.0), masking an input-tag error. The Lead
adopted the correction: both scalar tables now use untyped `key_value` and
`record_value`; UUIDv7 `record_key TEXT NOT NULL PRIMARY KEY` remains. Literal
expected values, scalar tags, receipt replay and physical reopen assertions
are unchanged. No permanent mutant test or product behavior was added.

The permanent native `numeric_composed` integration case and real workerd
`composed-numeric` mode exercised the corrected fixture. Both mutants ran
only in private archived source derivatives; each changes the submitted
`record_value` while retaining the original expected snapshot:

| Input | Native | Workerd |
| --- | --- | --- |
| Original integer extremes and Real(1.0) | 1 passed, 0 failed, exit 0 | Passed, exit 0 |
| Integer(1) where Real(1.0) is expected | 0 passed, 1 failed, exit 101; scalar-tag rejection | Exit 1; same scalar-tag rejection |
| Real(9007199254740992.0) where Integer(2^53) is expected | 0 passed, 1 failed, exit 101; scalar-tag rejection | Exit 1; same scalar-tag rejection |

Each workerd invocation stopped and reaped both owned processes, including
the failing derivatives after reopen. The Worker artifact is reused deliberately:
this fixture is client-only and the owning runtime/receiver code is unchanged.
Its SHA256 stayed byte-identical before and after all six required observations:
`3548c7a604cb10bae99f4b1d18c424166f1611ee0fceeb390b60bb36b51b175d`.
Corrected fixture SHA256:
`ebcd957ddb474b7557f20c085bd3790a118937c80ad24732f7ec4aa497e58497`.

The earlier 6523108e gates remain historical evidence; they did not establish
input-tag sensitivity for row 8. The Lead accepted rows 4, 7, 18, 32, semantic
coverage for 29, coverage for 30, retirement of 31, the four controls and release
isolation at that checkpoint. Independent re-observation of this corrected
row 8 belongs to Verifier B after the Lead declares the new head. The d/e/f
rows remain unchanged and incomplete.

The Lead reports the exact current-fence-epoch library regression was added
at 77da7d4a (decision 496). This lane remains pinned to 68394e06 and does not
claim that later library proof; it arrives through B's integration.

Fresh quality checks for this correction passed: `cargo fmt -p
collaboration-central-conformance -- --check`, `git diff --check`, and
`cargo clippy -p collaboration-central-tokio --features conformance --test
numeric_composed -j4 -- -D warnings`, each exit 0. After the six required
observations, both ordinary clients were rebuilt from the corrected working
source and passed again (native 1/0 exit 0; workerd exit 0), restoring the shared
target's artifacts after private-derivative execution. These restoration runs
do not add new scenario coverage. All eight owned workerd processes across
four invocations stopped and reaped; all build/runtime commands are settled.
The other 32 ledger rows remain byte-identical to the prior checkpoint, as do
the unchanged expected snapshot functions in both private derivatives.
