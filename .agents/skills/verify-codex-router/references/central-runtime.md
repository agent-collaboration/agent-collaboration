# Central runtime

Observe central through explicit retained one-core conformance entries on Tokio
and local workerd. A completed run means its selected scenarios completed;
the gaps and retired obligations in `run.json` remain open. It does not establish
all of OB6 or OB8, or equivalence to a retired aggregate.

The adopted oracle is the one-core Program Design §13 and the board Verification
Design OB6, OB8, OB9 and §5c–§5d at the commissioning design revision. The
§14 cut and decisions 508/513 retire the legacy runtime, launchers and controls;
decision 515 keeps dev conformance separate from production Worker builds. The
commission must supply those artifacts when they are absent from this checkout.
SO53/SO56/SO57 session cases also consume the consumer-fit VD at the declared
agent-fleet-sync revision. Take expectations from those sources, not test output.

## Preflight and target

Supply the complete source commit, a task-owned source root, and a fresh private
evidence directory outside the checkout. Python 3.13 and Pydantic 2.14.1 run the
apparatus. Native Cargo uses the repository's signing runner through `.cargo`.
No installed Router, Codex profile or production state is involved.

Build only in this worktree's normal Cargo target: native executables under
`target/debug`, with Cargo's normal cross-target debug child for Wasm. Take the
Implementer's explicit build-slot release before building; never build alongside
it. No per-run or scratch Cargo target is allowed, including for archived source.
The driver selects the worktree target from its own location. Archive independence
comes from pinned source and recorded hashes, rather than a separate build cache.

The driver refuses product changes in `Cargo.toml`, `Cargo.lock`, `.cargo`,
`crates/` and `scripts/tooling/`, including unwired untracked source. Keep
apparatus and proof-document changes separate from the declared product target.
When a baseline must remain stable while implementation continues, an explicitly
commissioned `git archive` snapshot is supported: supply its tar file through
`--source-archive` and its extracted root through `--repository`. The archive's
embedded commit and product file contents must match. Keep the archive and its
SHA256 beside the run. Never move or discard another writer's drafts.

For workerd, install the pins in this task's own tools root, not global Cargo bin
or another task's scratch. These commands come from the existing CI tool setup;
`--allow-build workerd` is needed by current pnpm to run its binary installer.

```sh
mkdir -p "$TOOLS/node"
BUILD_TARGET="$PWD/target"
df -h .
CARGO_TARGET_DIR="$BUILD_TARGET" CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo install worker-build --version 0.8.7 --locked --root "$TOOLS"
pnpm --dir "$TOOLS/node" add --save-exact --allow-build workerd miniflare@5.20261012.0-beta
```

Run from the worktree root. Set `TOOLS` to the caller-authorized tools directory;
its Cargo installation uses the same normal worktree target. The driver validates
worker-build 0.8.7 and Miniflare 5.20261012.0-beta, and records tool/package hashes
before the run. This
Miniflare pin selects workerd 1.20261012.1. The preflight records Node and workerd
versions and executable hashes too. `worker-build` may obtain its matching wasm-bindgen CLI;
keep that preparation inside the authorized tool/cache boundary.

## Run

Set `SOURCE_HEAD`, `SOURCE_ROOT` and `EVIDENCE` from the declared target, not an
old run. `EVIDENCE` must not exist. For an archive, add
`--source-archive "$SOURCE_ARCHIVE"` to either command. Choose
`--purpose baseline-characterization` for the old implementation and
`--purpose declared-target` for an Implementer's declared slice.

```sh
uv run --offline --no-project --with pydantic==2.14.1 --python 3.13 \
  python -m scripts.verification.central_runtime --runtime tokio \
  --expected-head "$SOURCE_HEAD" --repository "$SOURCE_ROOT" \
  --evidence "$EVIDENCE" --purpose declared-target \
  --package collaboration-central-tokio --features conformance \
  --test-target step_transactions
```

Native runs require explicit `--test-target` selections. The former default
aggregate and `local_storage`/`local_conformance` targets retired with the legacy
runtime (508); selecting them fails explicitly. Read the declared slice's offered
tests and name each retained integration target. Each must execute at least one
test. Counts retain passed/failed/ignored and filtered totals; empty or missing
suites cannot turn green. Never change selection merely to avoid a failure.

For the Native Registry notice producer (decisions 687/689/690/691), use three
separate invocations of the Native recipe with these exact target/name pairs:

| `--test-target` | `--test-name` |
| --- | --- |
| `process_restart` | `registry_notices::registry_claim_notice_survives_process_death_and_expires_without_a_read` |
| `owner_registry` | `notices::holder_changes_notify_the_previous_holder_and_regrant_without_renewal_duplicates` |
| `registry_lease_publication` | `notice_atomicity::mark_write_failure_rolls_back_notice_admission_before_delivery_and_retry_recovers` |

These are three Native cases. They require normal Claim's literal full grant
(epoch 2, four-hour expiry and seed start), positioned `LeaseGranted`, A→B→A
`LeaseSuperseded` at the prior epoch and re-grant at epoch 3, replay/renewal
without duplicate positions, and rollback of both journal admission and the
handled mark when its write fails. Producer keys contain database, epoch, kind
and installation; only Granted and Superseded are produced. Removing the mark,
skipping admission or omitting the epoch must fail their respective oracles.

The process case compares the owning `machine_notices` timestamps before and
after SIGKILL/restart: exactly seven days apart and unchanged before reconnect
or any clock command. It reads the retained page, then observes automatic expiry
through a read-only store connection before any later notice read. The receiver
creation clock alone is backdated so expiry falls near wall time; the restarted
process receives no SetTime command. Removing startup's persisted-expiry re-arm
must fail. This proves durable expiry behavior, not a persisted platform timer
row. Inspect distinct process IDs and the kill/wait barrier. Worker imported-Claim
smoke does not establish notice or expiry parity; row 28, hosted behavior and
whole-PR readiness require their own observations.

For Fleet notice recovery after process loss (legacy row 28; decisions
676/678/699/731), run the Native recipe twice with `--test-target process_restart`:

- `--test-name fleet_notice_outbox::archived_workspace_notices_recover_after_process_death_before_dispatch`
- `--test-name fleet_notice_outbox::archived_workspace_notices_recover_after_real_ack_loss_without_duplicate_positions`

Each case waits on actual dispatch hold or successful-reply loss, SIGKILLs and
waits its owner, then reads the stopped sender's original Running task, exact
input and digest before replacement starts. After normal reconnect, require one
BoardArchiving and one FleetFeedAdvanced(6) at positions 2/3, the original
operation/step's receiver receipt, unchanged snapshot/feed and business replay,
two terminal jobs, no pending debt, and no later duplicate. Repeated runs and
child lifetimes add no groups. A no-op drop must fail the loss-readiness timeout;
fresh receiver keys per Fleet attempt must fail on duplicate notice positions.

The acknowledgement-loss setup retains a next-dispatch hold through SIGKILL.
It pre-arms the hold and successful-reply drop, then uses the existing atomic
ResumeDispatchAndHoldNext until both the real dropped reply and next hold are
observed. The receiver's first asynchronous append can need another attempt;
freezing Fleet's clock before that successful reply prevents readiness. Releasing
the retained hold and waiting for completion before death must fail the original
pending-input guard. Repeat both cases, including within the concurrent full
`process_restart` target; repetitions check scheduling and add no distinct groups.

The new DropNextObjectReply/AwaitDroppedObjectReply control is conformance-only.
Both platform wrappers must call the shared reducer only after successful real
transport, leaving it armed across failures; only the first matching success is
swallowed and signals readiness with the original request. Check source gates,
the normal Native build and production Worker artifact exclusion for both tags.
Native physical-process proof and Worker imported-Claim smoke do not establish
active-control or whole-process Worker parity, hosted behavior or whole PR 3.

For staged Copy expiry across Native process death (row 33; decisions 708/709),
select `--test-target process_restart` and
`--test-name copy_staged_parts::copy_staged_part_expires_automatically_after_process_death_and_restages_freshly`.
Run the kept `--test-target copy_staged_alarm --test-target copy_staged_expiry`
as a separate Native invocation without a test-name filter: three alarm cases
and one explicit-alarm store-reopen case remain part of the proof.

The physical case normally creates the Fleet graph, Registers and stages a real
multipart Piece. After SIGKILL/wait and a fresh process, typed public owner Health
must show the same transaction/index/timestamp before its original deadline,
then an empty stage after it without SetTime, ApplyPiece, Repair or reconnect.
The unchanged Health read cannot prune or arm a wake. Exact original restage
must return PartStaged with a fresh bounded timestamp and unchanged complete
CopyStatus, schema and acknowledgement. The first Copy clock alone is backdated
to put its strict one-day deadline (86,400,001 ms) near ordinary wall time.

Removing only the existing-Copy startup wake and suppressing expiry discard must
each fail at automatic expiry after real restart. Fresh restage alone does not
prove timer arming: retain the component cases' exact per-part/earliest deadline,
young-part preservation and fence assertions. This is expiry behavior proof,
not a persisted private alarm-list assertion. The admin-protocol DTO import is
test-only. Worker/DO expiry parity, rows 2/3, hosted and whole-PR readiness remain
separate; reuse prior image evidence only after checking unchanged build inputs.

For lease publication and refused cleanup across Native restart (rows 2/3;
decisions 713/715/716/719), use separate exact Native recipe invocations:

| `--test-target` | `--test-name` |
| --- | --- |
| `process_restart` | `lease_publication::a_new_registry_run_gates_publication_and_fences_delayed_grants` |
| `process_restart` | `archive_cleanup::refused_claim_cleanup_recovers_after_process_death_before_dispatch` |
| `process_restart` | `archive_cleanup::refused_claim_cleanup_recovers_after_real_settlement_acknowledgement_loss` |
| `startup_controls` | `startup_refuses_non_arming_controls_before_discovery` |

The initial startup profile is conformance-only and arming-only; waiting,
resuming and observing commands must be refused before stores open. Prove its
marker present in the actual Native conformance executable and absent from the
normal no-default compiled library; Worker cannot prove a Native constructor's
exclusion. Ordinary start/start_on retain their default behavior.

Row 2 requires a real pre-armed first incarnation allocation, unchanged Directory
while publication waits, normal renewal, then current publication after release.
Label its subsequent stopped owning-Fleet-store check separately: exact Fenced
for a lower incarnation, unchanged full projection and still-Active reservation.
It is an actual Core/store boundary, not another physical transport assertion.

Row 3's real refusal is SchemaTooNew from Copy schema 1 and claimant Hello max 0;
only the old AlreadyHeld fixture/reason is retired. Keep both loss paths, original
Running input/attempt/digest, recovered call observed before release, exact
finalized actors/origin/request None/time/feed, unchanged full ledger, terminal
debt and later no repeat. ResumeDispatchAndHoldNext must arm under the existing
mutex before releasing the prior call. Four deciding wrong builds bypass the
startup gate, remove Fleet's lower-fence guard, skip refused settlement and force
a non-atomic handoff. Restore and rerun green. The shared control tag must be
present in dev and absent from production Worker images. Worker physical restart
or atomic-handoff behavior, hosted and whole-PR readiness remain separate.

For session resume after Native process death (row 6; decisions 728/731), select
`--test-target process_restart` and
`--test-name session_resume::lost_piece_and_renewal_answers_replay_after_the_server_process_dies`.
This one case normally creates the Fleet graph and Registers the enrolled peer,
drops the exact Piece and Renew frame answers after execution, and observes the
original four-hour renewal expiry in a separate Welcome without replay. Require
SIGKILL/wait before opening the same stores in a new process. Its typed Welcome
must retain acknowledgement 1:1 and the exact epoch, holder, Held state and expiry.
Identical Piece replay returns AlreadyApplied; SnapshotPage retains schema 1,
position 1:1, the original table and empty rows. Replay the original renewal
attempt twice: replayed true, no transfer, unchanged recorded expiry and authority,
with remaining lifetime decreasing as Registry time advances. Wrong Welcome
acknowledgements and fresh renewal keys must fail after physical restart; mapping
AlreadyApplied to Applied must also fail. Restore exact source before rerunning.
This is the test harness's real Native HTTP/socket/store/process boundary, not a
shipped central executable or full Router writer journey. Worker session/restart
parity, other legacy rows, hosted behavior and whole-PR readiness stay separate.

For row 5 group 1 and row 9's shared nonempty lost-Piece group (decisions
734/768/769), use `--test-target session_lost_row`. Its one Native case is
`lost_nonempty_piece_is_read_without_new_hello_and_reconciles_across_two_reconnects`.
This complements the separate physical-restart case above, whose lost Piece
contains only schema.

Normally enroll, create the Fleet graph, Register epoch 1 and apply schema 1
at prefix 1:1. Send a real row Piece through 1:2 with the literal UUIDv7 key
`019f0000-0000-7000-8000-000000000770`, integer 1 and label "accepted once".
Require the exact outgoing frame answer to be observed dropped. Before any
close, new Hello or reopen, read CopyStatus and SnapshotPage on that original
socket: fence 1, prefix 1:2, schema 1, exact database/table `lost_rows`, the whole
single row and no next page. Strict frame correlation must reject a stray
answer from the original Piece.

Then honestly close and reconnect with the same session identity. Require the
entire sole Ready entry: ack 1:2 and the original Held epoch-1 holder, four-hour
expiry and remaining lifetime. Replay the identical Piece as AlreadyApplied
through 1:2, then read the identical status and snapshot on that replacement.
A second honest reconnect must repeat the same Welcome, replay and reads.

Empty snapshot rows must fail on the original surviving socket after actual
answer-loss readiness; stale Welcome acknowledgement must fail after that
successful read; AlreadyApplied mislabeled Applied must fail after the exact
first Welcome. A control that signals loss readiness but forwards the old
answer must fail strict frame correlation before surviving-read credit. Restore
exact private source and rerun green.

Only the deleted actor-cache eviction operation is retired under 734. This
recipe adds no eviction, private hook or reopen substitute; its live read is
transport continuity evidence. Physical owning-process persistence is separate,
and Worker/DO eviction or hibernation is unobserved. Remaining row 5 groups,
full rows 9/10/12, Router integration and full gate 737 remain separate.

For row 5 group 2's interrupted owner Takeover (decisions 769/785), use
`--test-target owner_takeover_recovery`. Its one Native case is
`takeover_replays_committed_lost_fence_after_owning_reopen_without_second_epoch`.

Use the same normal enrolled/Fleet/Register/schema fixture throughout. Take
Preview at 1:1, apply the literal UUID-keyed row 2 through 1:2, and require the
stale Preview refusal with unchanged Copy fence 1 and exact row. The stale
attempt reserved epoch 2; under 785 the next Takeover must use epoch 3 and
start 1:2, with no historical grant at epoch 2. Do not reset or split the fixture
to recover the retired numeric-2 expectation.

Hold the actual Registry-to-Copy Fence and check its typed target, database,
transfer, CallId, new epoch 3, target holder, expected epoch 1/ack 1:2 and schema
support. Lose that exact successful reply after the real Copy call. Require the
same public Pending transfer and Copy fence 3/target holder/retained row 2 before
reopen. After owning server shutdown, the test-local grant_readback module uses
only public fleet_sync_lease::epoch_holder on an existing registry.db opened
read_only: exact grant 1, no grant 2 and no grant 3. It rolls back its read
transaction and closes before a new server starts; no private SQL is needed.

Start a fresh owning server/store on the same root, with Registry time advanced
one second and the existing Copy-dispatch hold pre-armed. Require exact equality
of the full decoded original Fence envelope, including CallId/request/target, then
resume. The same owner attempt must grant and replay exact epoch 3/start 1:2.
Old-holder Piece must return exact StaleEpoch 3/cutoff 1:2 with no rejected tail;
target Piece 3:1 must apply. Both sockets read the complete row 2 plus new-holder
row 3, exact fence/holder/schema/prefix and no extra page. After final owning
shutdown, public history must retain original grant 1 ended at 1:2, target grant
3 at the pinned second time, and no grant 2.

Successful-but-uncommitted Fence, wrong replay receipt, old-writer false success
and missing new snapshot row must fail their actual-stage assertions. Restore
exact private source and rerun green. A forgotten-receipt experiment that merely
times out cannot be credited as the intended literal grant assertion or as a
normal product defect.

This proves Native owning-server/store reopen in one test process. It is not
OS SIGKILL recovery, Worker/DO eviction or hibernation, a whole redeploy fixture
or Router integration. Row 5 groups 3–5, full rows 9/10/12, full gate 737 and
final integrated/hosted readiness remain separate.

For row 5 group 3 (decisions 769/792/794/799), select these three Native targets
together with no test-name filter: `transfer_schema_support` (four cases),
`transfer_floor_repair` (three), and `transfer_ordered_rows` (two). This is nine
cases, separate from the three accepted `owner_fence_ordering` overlap cases.
Reuse overlap only after checking its exact test/helpers/runtime/build inputs.

Before each run, read the actual host `netstat -an -p tcp` line count. Above
about 15,000, wait for pressure to clear; do not churn builds or change network
policy. Reject a sandbox denial or stderr even if netstat exits 0 with no rows.
AddrNotAvailable before scenario setup is an environment failure, not a product
result. A historical host-permission success does not prove permission causation.

Schema cases use distinct real producers: known Hello max 0, never-enrolled
installation, genuine known enrollment with no Hello/report, and correlated
holder Release refusal. Require exact SchemaTooNew(Some 0), NotFound,
SchemaTooNew(None) and holder SchemaTooNew respectively, including admin replay.
The known/no-Hello case must reach a real held Fence with target_schema=None.
Every case keeps full Copy fence 1/schema 1/prefix 1:1/empty snapshot unchanged.
After owning shutdown, the read-only public epoch_holder reader requires the
exact original grant and no grants 2–6. Only the old never-enrolled
SchemaTooNew(None) label retires under 794; genuine unknown support stays tested.

Arrange Copy-only fence 5 through the existing public CopyFence conformance
route. Preview at Copy5 must select the first Fence6 with step `fence`.
Takeover from an older Preview must first send Fence2, then retry Fence6/from5
with the same operation and step `fence:2`. Move must first receive its exact
correlated Release, hold it while Copy advances to 5, acknowledge final 1:1 via
the actual CopyAcknowledged call, then observe the same Fence2→6 repair. Each
must grant/move and replay exact epoch 6/start 1:1, retain the empty Copy,
refuse the old writer at 6/cutoff 1:1, and install the target's entire usable
Registry lease 6. The older-Preview case also checks old-writer StaleEpoch 5
before transfer. Never arrange this through a private Registry-floor edit.

Ordered cases retain literal UUID-keyed rows with numbers 1/2: before-left/right
commit and are completely read at 1:2 while exact Fence2 is held, then the Fence
refuses PreviewStale and Copy stays unchanged. In the other order, grant2/start
1:1 precedes the whole late-left/right Piece, which must be StaleEpoch2/cutoff
1:1 with empty Copy. Target Piece2:1 commits both new-left/right rows, and both
target and old sockets read the complete new snapshot with no late rows.

Unenrolled relabeling, unsupported-schema admission, altered holder refusal,
wrong Preview/retry epoch selection and partial ordered snapshots must fail at
their real-stage assertions. Restore exact private source and rerun all nine.
Keep filtered/repeated sensitivity counts separate from unique cases. An earlier
ordered-only lint revision does not change separate schema/floor target inputs;
bind that precise unconsumed delta before reusing such evidence.

This is Native fixture-client HTTP/socket/core/store composition. DAG envelopes,
public missing-Copy/no-grant, full rows 5/9/10/12, OS recovery, Worker/DO,
Router integration and full gate 737 remain separate.

For row 5 groups 4–5 (decisions 769/806/807/808), keep these six cases distinct:
one Core typed edge test, three Core decoder unit tests, one direct Registry
on_call case and one Native missing-Copy recovery case. For the typed test,
use the Native recipe with `--package collaboration-central-core --test-target
call_edge_mapping`. For decoder units, run from the declared source:

```sh
cargo test --locked --offline -p collaboration-central-core --lib \
  object_transport::tests:: -- --nocapture
```

The production decoder is tested by an in-crate cfg(test) module; no public
test export is needed. Require typed CallEdgeRefused{Copy, Registry} with
Copy→Fleet allowed. Direct decode must refuse well-formed Copy/Object→Registry,
and allow Session/Object→Registry plus Copy/Object→Fleet. Removing only outer
may_call must fail the decoder case (2/1/0) while direct public on_call still
passes (1/0/0): the inner Registry handler independently refuses it. Record
that masking as defense in depth, not isolated decoder proof. A changed build
that refuses everything must fail both allowed decoder controls (1/2/0).

For public on_call, use the Native recipe with `--test-target machine_enrollment`
and exact `--test-name call_edge_mapping::registry_public_call_refuses_copy_authority_and_allows_session_peer_read`.
This direct runtime/store fixture requires Copy refusal and a permitted Session
peer read returning serialized None; it is separate from HTTP/whole-network proof.

For missing Copy, use `--test-target transfer_schema_support` and exact
`--test-name missing_copy::missing_copy_after_preview_is_unavailable_without_grant_then_same_takeover_recovers`.
Create/Register/schema 1 and Preview normally, then gracefully stop all owners
before moving only this test's Copy database and present WAL/SHM files outside
the scanned root, retaining exact bytes. Reopen with the same Registry root.
Require session CopyUnavailable(Uninitialized), owner HTTP 503 typed Unavailable
for the same transfer twice, and a fresh target Welcome with no lease.

After owning shutdown, read_only public receiver copy_status must be exactly
NotInitialized. Public lease_status and epoch_holder must match the original
lease/grants, with no grants 2–6; holder_change_answer must be Pending for the
same full request. These are distinct public/domain observations, not private
SQL or a synthetic marker. The old same-live-socket Renew/no-active-transfer
condition is retired under 807; the earlier drafted session NotInitialized
expectation is corrected under 808 and supplies no product-failure credit.

Stop owners before restoring: remove only newly created empty files for that
one Copy, check saved bytes, rename them back. Before retry, the public complete
original CopyStatus must retain fence 1/holder/schema/prefix. Advance only the
existing Registry fixture clock five seconds for the real retry deadline.
The same full Takeover must grant epoch 2/start 1:1. Require actual Copy fence
2/target/prefix and stopped public Registry lease/history/outcome 2, no grants
3–6. A changed build replacing retryable absence with terminal refusal, or
misreporting recovered epoch 3, must fail the corresponding exact assertion.
Restore exact private source and rerun all six unique cases.

File loss is limited to owned temporary files after shutdown. This is graceful
server/store reopen, not OS crash, Worker/DO or Router proof. Private floor loss
remains library-owned. Whole row 5 acceptance, row 9 revocation mapping in full,
rows 10/12, full gate 737 and final integrated readiness remain separate.

For row 5's renewal-history subgroup (the old renewals_and_supersession function;
decision 734), use `--test-target session_renewal_history`. Its two cases are:

- `lost_renewal_replay_fresh_extension_and_takeover_keep_recorded_and_current_separate`
- `expired_welcome_is_restored_by_a_fresh_same_epoch_renewal`

Both normally create, Register and apply schema 1 at prefix 1:1. The first loses
the exact committed R1 frame answer, reconnects 15 Registry minutes later and
requires the entire Welcome lease to retain the original four-hour expiry.
Replayed R1 remains unchanged; fresh R2 extends expiry by 15 minutes. After real
owner Takeover and exact grant replay to epoch 2, R1 still reports recorded
epoch-1 success and its original expiry alongside current epoch-2 target
authority. A fresh old-holder attempt must return StaleEpoch 2/cutoff 1:1.

The separate fresh-fixture expiry case reconnects after five Registry hours:
Welcome must show Expired, original expiry and zero remaining. Fresh renewal
restores Held in the same epoch, with literal expiry nine hours after the start
and four hours remaining. Keep full typed lease/result equality, transfer=None,
replayed flags and unchanged schema-1/prefix-1:1/empty snapshot assertions.

Wrong fresh keys, collapsed recorded/current fields after Takeover, and Expired
Welcome forced to Held must fail after the actual drop, Takeover and clock-stage
markers respectively. Restore exact private source before rerunning green.
This proves two Native cases within one old subgroup. Other row 5 groups,
surviving-socket/in-flight replacement, rows 9/10/12, Worker parity and OS
recovery remain separate, as do final restack and whole-PR readiness.

For row 5's Move subgroup (decisions 734/750/751/752), use
`--test-target session_move`. Its five Native cases are:

- `move_keeps_one_transfer_and_waits_for_the_correlated_holders_exact_final_prefix`
- `pending_holder_migration_is_a_typed_replayed_move_refusal_without_a_fence`
- `failed_holder_migration_is_a_typed_replayed_move_refusal_without_a_fence`
- `stale_renewal_age_preserves_its_exact_time_and_differs_from_delivery_failure`
- `a_disconnected_recent_holder_is_really_unreachable_and_keeps_the_copy_unchanged`

Require normal creation/Register/schema 1, the exact active TransferInProgress
and renewal transfer notice, and the correlated holder ServerCall::Release.
Released final prefix 1:2 must lead to the exact held CopyAcknowledged payload
and durable await_acked identity. Observe one real waiter below that prefix,
unchanged Copy at 1:1, then apply the drained UUIDv7-keyed row. Require Move at
epoch 2/start 1:2, two exact replays, the retained row and exact target Renew.
Migration 2 Pending and Failed must each retain their typed refusal/replay,
fence 1, prefix 1:1, empty rows and original lease without an active transfer.

Keep decision 752's existing board wire: stale renewal is holderUnreachable
with Renewal/lastRenewedAt, decoding to native HolderStale with the exact time;
actual detached recent-holder delivery failure is Presence/lastSeen, decoding
to native HolderUnreachable. Assert literal shapes and native decoding. The
fixture checks actual socket detachment, uses release_unsent_for=0 and advances
Registry time 10 ms after the first Pending attempt, matching the engine's
initial retry and strict unsent boundary. This is not a production timeout proof.

Lost transfer notices, bypassed Copy waits, changed migration detail and swapped
observations must fail their corresponding actual stages. Restore exact private
source and rerun green. The holder is a fixture WebSocket; this is Native central
composition, not underlying Router writer/migration execution. Other row 5
groups, rows 9/10/12, Worker parity, OS recovery and final restack gates remain
separate. A's board DTO is unchanged.

For row 5's in-flight replacement portion (old `session_scenarios.rs:509–553`;
decisions 734/750/758), use `--test-target session_inflight_replacement`.
Its one Native case is
`exact_held_renewal_resends_on_the_same_session_replacement_and_then_replays_exactly`.

Reuse normal Fleet creation, Register and schema 1 at prefix 1:1. Advance
Registry time 15 minutes, hold the actual Connection-to-Registry dispatch and
require its full RenewRequest, kind, source, face, destination and authenticated
peer. Extract the actual held session identity. An authenticated Hello with that
same identity must return the entire original Ready entry: prefix 1:1, epoch 1,
holder, original four-hour expiry and 13,500,000 ms remaining. The old connection
must receive typed ConnectionReplaced followed by physical close.

Resume dispatch and safely resend the same R1 on the replacement. Require the
exact settled recorded epoch 1/expiry T0+4h+15m, current original holder/Held
lease with four hours remaining, and no transfer. The first resend's replayed
flag is contingent; do not require either value. The next same-R1 answer must
have replayed=true with that exact receipt and current lease. A SnapshotPage on
that same replacement must retain database, schema 1, prefix 1:1, move_records,
empty rows and no next page. Keep strict frame correlation throughout.

Native deliberately aborts and drains the replaced socket's request futures
in `native_objects.rs`; the original call's completion and committed fate are
not this oracle. Fresh actual Registry renewal keys must fail exact replay;
a different typed replacement close reason must fail after the new Welcome.
Sending a different valid renewal into the hold must fail full attribution
before the held-request credit marker. That last check is fixture sensitivity,
not another product case. Restore exact private source and rerun green.

This is one Native HTTP/WebSocket/core/store case with a fixture client. The
credential portion at old lines 554–637, the rest of row 5, rows 9/10/12,
Worker parity, physical process recovery, real Router integration and final
restack/whole-PR gates remain separate.

For row 5's credential portion (old `session_scenarios.rs:554–637`;
decisions 734/750), use `--test-target session_credentials`. Three Native cases
cover revoke, machine expiry and audience/unknown refusals:

- `revoked_registered_machine_gets_typed_physical_close_and_cannot_upgrade_again`
- `expired_machine_credential_closes_on_frame_and_refuses_a_fresh_access_verified_upgrade`
- `unregistered_token_owner_upgrade_and_machine_admin_are_refused_without_poisoning_the_live_holder`

Reuse normal enrollment/Fleet/Register/schema 1 at prefix 1:1. Before revocation,
capture the actual session ID from an exactly attributed held renewal, resume
and complete that request, then authenticate the same-session replacement.
Require its whole Ready Welcome, typed/physical old ConnectionReplaced close
and successful exact renewal on the current replacement. Owner Revoke must
return literal MachineRevoked, then that replacement must receive exact
CredentialRejected(Revoked) and physically close. A fresh Access-signed upgrade
with the same credential must return 401/Revoked.

The enrollment helper verifies the stored expiry `2033-05-18T03:33:20Z`.
Advance Registry and the old Connection clocks one millisecond beyond it and
require both exact readbacks. A real Renew frame must receive typed/physical
Expired close, then a fresh Access-signed upgrade must return 401/Expired.
The edge's Access clock stays at normal wall time: this checks stored machine
expiry, not JWT expiry, wall-time passage or a universal credential lifetime.

An un-enrolled token with its machine Access assertion must return 401/Unknown.
Owner machine-upgrade and valid-machine admin takeover/preview must each return
401 with literal {"kind":"wrongAudience"} and typed AccessRefusal::WrongAudience.
Keep exact live-holder renewals before/after refusals, and unchanged Copy
fence/schema/prefix/empty snapshot after each case. Wrong frame clocks, bypassed
expiry/revoked upgrade gates, wrong revoked close, unknown classification and
audience bypass must fail their corresponding real-stage assertions. Revocation
controls must first reach the usable replacement marker. Restore exact private
source and rerun green.

These are three Native fixture-client cases. The un-enrolled string is not an
actual Router agent-token producer. Remaining row 5 groups, rows 9/10/12,
Worker/DO parity, OS restart, Router integration and final integrated gates
remain separate. Do not credit an earlier test revision that lacks replacement
before revocation or the tagged WrongAudience literal.

For row 9's full revocation mapping (old staging_scenarios.rs:283–316;
decisions 768/813), use the same `--test-target session_credentials` recipe.
It still has three cases: one existing revocation case is extended and two
unchanged cases rerun. Keep every previous revocation assertion; the extension
adds only an unrelated-installation reconnect after the old checks.

After the live replacement receives Revoked, physically closes, and its fresh
upgrade is refused with 401/Revoked, require the independently enrolled target
to perform a fresh Access-signed HTTP 101 upgrade. Send a new SessionId Hello
and require that exact ID with an empty Welcome. Move the actual new socket
into the fixture's target ownership and read the unchanged Copy through it:
fence 1/schema 1/prefix 1:1, original table and empty snapshot. A surviving old
target socket or an upgrade alone does not establish this reconnect outcome.

Changed builds that should fail include an altered live Revoked close, bypassed
revoked upgrade admission and a revoked record from one installation wrongly
poisoning another installation's fresh authentication. The last must reach the
prior revocation/close checks, then fail the unrelated upgrade with 401/Revoked.
Restore exact private source and rerun all three. Preserve the prior function
bytes after removing only the added helper call; test-only field visibility is
not a product API change. This is local Native fixture Access/JWKS and real
HTTP/WebSockets, not hosted Access, Worker/DO, OS recovery or Router proof.
Earlier accepted shared row groups are not a fresh combined run; whole-row
acceptance, rows 10/12 and full gate 737 remain separate.

For row 12's original-socket read on the imported normal fixture (decisions
821/824/826), use `--test-target normal_redeploy --test-target archived_claim`.
The 39dda133 checkpoint has two cases: one new normal-session case and the
unchanged archived-Claim/default-seed regression. Arrange the two projects through the
existing Native public ImportTask and Copy seed before server startup. This
setup does not exercise D's HTTP import. Keep imported head 1:3 and every old
integer id and label; carry each logical id 1–9 under its fixed UUIDv7 TEXT
record_key in the full rows, upserts and deletes.

Preserve the complete old normal prepare: both Claims at epoch 2/start 1:3,
multi-row changes, stale Preview refusal, exact rows at 2:2 and the still-writable
2:3 Piece. Require actual Takeover 4/start 2:3 and identical replay, and the old
writer's StaleEpoch newer 4/cutoff 2:3. Keep the other project's independent
writes, unread 2:2 answer, honest reconnect and original 2:2 receipt after its
later 2:3 update. Stale Preview reserved 3 without granting it; retire only the
old numeric 3, preserving the old positions and logical rows.

Send B's in-flight Piece 4:1→4:2 without reading its answer. On that same
original B socket, send a fresh SnapshotPage frame before any replacement B
Hello. Skip only notices and the original in-flight answer. Require the new
frame, database, records table, schema 1, no next page and either complete
prefix: 4:1 with six rows or 4:2 with seven rows. Compare full key/id/label rows.
The existing read-only schema-report observer must report the original peer,
session, attach 1 and support 1 both before and after this read. That observer
adds a guard; it never replaces or changes the complete-prefix oracle.

Then perform the honest same-session B reconnect, replay the saved Piece through
4:2, write row 9 at 4:3 and compare all eight full rows. Replay the saved owner
Takeover 4/start 2:3 and reconnect the old writer to require the same stale4
cutoff2:3. After successful shutdown, open the existing Registry read-only and
call public epoch_holder(database, 3), requiring None. Roll back and close the
reader; this check does not assert the entire grant history.

Changed builds that should fail remove a row at 4:2 or 4:3, respectively failing
the original-prefix or final-rows assertion after the preceding stage. A fixture
that sends an early real replacement Hello must fail the attach/schema guard
after the complete prepare. Conversely, a changed response presenting the
allowed whole 4:1/six-row prefix in place of 4:2/seven rows must pass; require its
marker so an unreached change earns no credit. Restore exactly and rerun both
cases. Check host TCP pressure before loopback runs and hold above about 15k.

This is Native HTTP/WebSocket/core/store proof with fixture Access/JWKS. It does
not stop and reopen the owning server: row 10, Worker/DO and OS recovery remain
separate, as do HTTP import, final pin/Router integration and full gate 737.

For row 10's complete local server reopen (821/824/826/832), use that same full
`normal_redeploy` plus `archived_claim` recipe. At 594f41e8 it executes three
cases: new `staging_normal_rows_pass_across_local_server_reopen`, retained row 12
and unchanged archived Claim. Select the new case with `--test-name` only for
its focused changed-build checks; the full restored run must include all three.
The seven independently seeded projects use the same public pre-start setup
and UUIDv7 key/id/label mapping. The main pair carries the full normal state;
the other five isolate the following original preludes.

N1 must run owner Move HTTP concurrently with the holder's Release response.
Require exact database/epoch 2/transfer fields, a normal old-holder Renew showing
that transfer still active with the old holder at epoch 2, and the correlated
Release answer at 1:3 while live. The same owner request grants 3/start 1:3 and
replays identically. A whole multi-change old Piece must get StaleEpoch 3/cutoff
1:3 with imported rows unchanged, then the new holder writes both moved rows
at 3:1 and the complete four rows match. Decision 832 retires only the old
sequential Pending-before-Release ordering and separate Released-ack carrier.

N2 compares the actual origin installation and label, project and opaque post
at the holder, then the exact opaque reply on the origin's original frame and
the unchanged full Copy. The holder's literal opaque answer remains the old
transport-only stand-in: it proves neither board admission nor two physical
machines. Real Router G3 closes that separate consumer-integration gap.

The owner-response-loss prelude requires machine-authenticated Move and Takeover
denied with Copy unchanged. Drop the successful owner response without reading
its body, then require two exact grant-3/start-1:3 replays. Old Preview must be
stale, a changed body must conflict, the new holder writes row 4, and an old
Piece gets exact stale3/cutoff1:3 with all four rows retained. S5 sends R1 without
reading, honestly replaces its socket, requires replay=true with the original
epoch-2 expiry, then fresh R2 replay=false with expiry at least that first one.
After actual Takeover 3, R1 still has its original expiry and current target at
3; a fresh old renewal is stale3/cutoff1:3. Keep the imported rows unchanged.

S6 reads the whole imported 1:3 prefix on the original attachment, then reads
again without another Hello. The local caller's window is Duration::ZERO;
this establishes no elapsed-time, timer, hibernation, eviction or new-instance
claim. Require the existing read-only schema report to retain exact peer,
session, attach 1 and support 1 around the second-frame path. A real extra
Hello that replaces this socket must fail the final original-attachment guard.

Only after all five preludes, run the complete normal prepare described above.
Require successful first-server shutdown, then actual original-B stream end
within ten seconds under the old None/Close/transport-error rule. The closed
state carrier must retain no old Platform/store/engine. Start a distinct new
TokioServer on exactly the same root. Through fresh authenticated reconnects,
require the saved in-flight 4:2 replay, new row 9 at 4:3, all eight full rows,
saved owner grant-4/start-2:3 replay and old stale4/cutoff2:3. Stop the second
owner before the same public read-only no-grant-3 check for the main database;
other prelude databases legitimately grant epoch 3.

Changed builds that should fail alter the Move prefix, forwarding label, owner
grant after unread response or renewal replay bit. A row removed only from the
4:3 read in the selected reopen case must fail after all preludes, complete
prepare, observed first-server/socket shutdown and second-server start. Check
the prior-stage markers, run the distinct S6 extra-Hello control, restore exact
private source and rerun the full three cases. This is graceful local owning
runtime/store teardown and reopen inside one test process; Worker/DO, hosted,
OS/process death, two machines, HTTP import, final pin/Router integration and
full gate 737 remain separate.

For connected Fleet cost observations (decisions 894/896), run these existing
integration targets from the declared source root, using the normal worktree
Cargo target and the preflight/environment above:

```sh
cargo test --locked --offline -p collaboration-service \
  --test read_cost_counter_contracts -- --nocapture --test-threads 1
cargo test --locked --offline -p collaboration-service \
  --test bounded_fleet_writes \
  fleet_write_reads_only_its_operands_and_survives_an_unrelated_corrupt_cache_record \
  -- --exact --nocapture --test-threads 1
cargo test --locked --offline -p collaboration-service \
  --test bounded_gate_reads \
  covered_reads_and_session_context_have_operand_bounded_cost \
  -- --exact --nocapture --test-threads 1
```

Require three classifier cases and one case per consumer. Repeat the two
consumer singles and retain each printed small/large cost. Run the complete
declared counter-consumer inventory in parallel with Nextest's CI profile,
four build jobs, four test threads and no retries. Obtain the inventory by
checking every integration binary importing support/read_cost_counter.rs;
include all cases in those binaries. Record the selected binaries, exact case
names and counts. This scoped parallel run is not a full-package gate.

The counter excludes a data statement only when its parsed table identities
are nonempty and all begin with _fleet_sync_. Mixed SQL, nested board reads,
multi-statement mixed input, unknown SQL and non-data commands remain counted.
This is a process-wide measurement window with that explicit bookkeeping
exclusion, not causal attribution to an operation. SQLx's complete db.statement
or its equal full-query summary supplies the parser input.

For the workspace-update journey with 8 versus 80 unrelated workspaces, require
small==large and exactly 57 non-bookkeeping statements. Exact body inputs are
Core canonical 2, Core feed 4, Router Fleet records 2 and Router decision 1.
Core counts are existing SQLx rows_returned for the named canonical/feed SQL;
Router counts observe opaque BoardPayloads in real Access-bridge replies.
Neither metric counts actual deserializer invocations. The payload observer
belongs only to conformance test support; no production MCP, Tokio or Core
instrumentation follows. Keep the selected-operation result and unrelated
corrupt-cache refusal assertions as well as cost equality.

On private source only, add a real Core high_water read before ReadFleet's
state read: the constant extra query must fail the exact statement budget even
when small==large. Add a real cache.snapshot read before refresh_cache's
checkpoint: growing unrelated bodies must fail cost equality. Switch the
classifier's all predicate to any: the mixed library/board JOIN must fail the
classifier contract. Retain each diff, compiled failure and intended assertion,
restore every byte, then rerun the affected retained targets green.

The two connected directory-refusal selections are board_control_path's
control_board_failures_pagination_and_inbox_use_the_public_path and
board_lifecycle_contracts' thread_root_failures_stay_specific_and_directory_collisions_create_no_native_file.
Run each exact case through the same cargo test recipe. Require normal central
NameConflict with the existing project identity, identical keyed replay and,
in the lifecycle case, no native project file before or after replay. A manual
settle_directory_decision injection supplies no connected-refusal proof.

These observations cover local real MCP/SDK, central and storage fixtures plus
the pure classifier rule. Worker/hosted behavior, installed Router, remaining
startup paths, final restack and the full delivery gate remain separate.

For durable Accepted registration recovery (decision 869), observe the Host
factory while the generic Fleet fixture refuses Read with typed CursorInvalid.
Require local outbox recovery before pending sends and refresh, and a separate
FleetClient caller returning exact InvalidReply with field fleetCursor. A
coalesced call returning Ok cannot establish the error result. The fixture may
supply typed directory/cache inputs, but must not return a fabricated feed Page
or manually start the native registration used as proof. In the Host case the
actual registration gate must be observed before projects.ensure.

The affected Host selection is four existing component cases. Run its exact
Nextest expression, with the CI profile, one test thread and no retries:

```sh
cargo nextest run --profile ci --locked --offline -p codex-router-host \
  --features keychain-test-support --lib --no-fail-fast --no-tests fail \
  --test-threads 1 -E 'test(=collaboration_runtime::board_composition::tests::native_registration_refusal_moves_an_empty_file_and_replays_the_original_reason) | test(=collaboration_runtime::board_composition::tests::accepted_directory_registration_waits_for_welcome_and_grant_before_the_first_record) | test(=collaboration_runtime::board_composition::tests::attaching_server_calls::native_release_and_withdrawal_are_answered_before_welcome) | test(=collaboration_runtime::board_composition::tests::a_machine_session_resumes_its_identity_without_bootstrapping_a_project)'
cargo test --locked --offline -p collaboration-service \
  --test fleet_registration_start -- --nocapture
```

The fresh-decision case observes the registration port while real Access,
central admission, session and durable settlement run. A newly decided send
must start registration inside settle_central_reply; replay must add no extra
start. That spy does not establish native registration by itself; the Host
factory cases supply that layer. Keep the zero pre-grant records/write gate,
first native transaction/Copy and refused-file orphaning, exact refusal receipt
and reopen assertions. The generic fixture's directory policy remains outside
these Host component cases.

An old-order private build that recovers only after refresh must fail at the
native-registration deadline with Read still refused. Swallowing the refresh
error must fail the separate-caller error assertion, and bypassing settlement's
start must fail the fresh-decision port observation. Restore exact source and
rerun the affected Host and fresh-decision cases.

For paused-owner fixture corrections, retain the separately declared exact
timing/recovery selection rather than broadening to the whole crate. Real
central setup may use the bounded blocking guard matching its existing 20-second
budget; it must end before owner timing, and additional-project setup must close
its temporary real session too. Keep the identical-Instant setup assertion and
the real SQLite query/virtual-timeout and auto-advance cases. Preserve every
existing paused attribute and deadline. Owner-domain fixtures retain their
existing typed grant arrangement; their pass count is not native registration
or real provider-delivery proof. Full Router startup, process G2/G3, installed
binary, Worker/hosted and full gate 737 remain separate.

For a declared RSA-profile target, `--test-target local_access --test-name
valid_2048_and_tampered_controls_do_not_admit_a_valid_1024_bit_signature` selects
the exact native real-verifier case. The driver adds `--exact`, records filtered
tests separately and still refuses zero execution. Use a separate run for the
declared core-policy/host/journal targets; the policy fixture's controlled crypto
does not replace this real native case.

```sh
uv run --offline --no-project --with pydantic==2.14.1 --python 3.13 \
  python -m scripts.verification.central_runtime --runtime workerd \
  --expected-head "$SOURCE_HEAD" --repository "$SOURCE_ROOT" \
  --evidence "$EVIDENCE" --purpose declared-target \
  --worker-build "$TOOLS/bin/worker-build" \
  --miniflare "$TOOLS/node/node_modules/miniflare/dist/src/index.js" --port "$PORT"
```

`PORT` is an available unprivileged loopback port. This conformance-only runner
builds the Worker with `--dev --no-opt -- --locked --features conformance-faults`.
It adds no production build or size-measurement path. Production builds outside
this runner must use `--profile worker-release --no-opt -- --locked
--no-default-features` (515); do not switch conformance runs to that profile.

The default Worker selection is `composed-transactions` plus `composed-numeric`.
Use repeated `--workerd-mode` flags for another explicit selection:

| Mode | Retained entry and scope |
| --- | --- |
| `composed-copy-suite`, `composed-transactions`, `composed-numeric`, `runtime-controls`, `composed-fleet-conflict` | `collaboration-central-tokio --features conformance --example composed_scope_workerd -- MODE NODE MF PERSIST PORT`; shared core scenarios, with physical reopen where the scenario requires it |
| `fleet-receipt` | `step_scope_workerd -- NODE MF PERSIST PORT`; owner-verified `/admin/v1/conformance/fleet-receipt`, ten checked-step groups |
| `bootstrap` | `bootstrap_scope_workerd -- NODE MF PERSIST PORT`; owner-verified `/admin/v1/conformance/bootstrap`, fifteen schema/enrollment fixture cases |
| `access-sessions`, `access-carryover`, `session-carryover` | `session_scope_workerd -- NODE MF PERSIST PORT MODE`; explicit normal-session mode last |
| `session-capabilities` | The same normal-session entry; actual workers-rs attachment integer/origin roundtrip and the same Connection object's competing-deadline alarm; two groups |
| `session-failures` | The same entry; targeted postcommit attachment error/physical close, healthy installation, correlated Register refusal, callback and SQL rollback with journal readback, ordered barrier and real retry; five groups |
| `holder-lifecycle` | The same entry; normal holder readiness/expiry/replacement, exact frame/connection, close/alarm/late response and held schema publication; five groups, with the fifth explicitly split into hub primitive and changed-SessionId wire close |
| `archive-reports` | The same entry; retained exact archive report/replay and Fleet board/workspace/feed/ledger oracle; one group |
| `imported-claim` | The same entry; typed Open import fixture, Open-board retirement refusal with seed unchanged, then exact Claim/replay/AlreadyHeld, Directory and Copy fence; two groups |
| `claim-archived` | The same entry; shared Native/Worker oracle, actual Archived unretired imported seed, exact normal Claim refusal and owning Fleet reservation absent before/after; one group |
| `claim-retiring`, `claim-retired` | The same entry; real owner retirement state versus cached Register/Claim and fresh Claim; one group per selector |
| `forward-origin`, `forward-disconnected`, `forward-reconnect` | The same entry; shared Native forwarding oracles with normal enrollment/Fleet/Register, literal origin/opaque body/reply, stored presence or original reconnect correlation; one group each |
| `forward-capacity`, `forward-deadline` | The same entry; reserved sixteenth slot/seventeenth Overloaded and terminal reuse, or original Unanswered/late-reply isolation; one group each |
| `forward-reopen` | The same entry with two owned process lifetimes on the same persistence; known correlation presence/body absence, retained sixteen slots, expiry and sixteen fresh deliveries; one group, at least two reaps |
| `raw-submit-refusal` | The same normal-session entry; permanent correlated raw Submit refusal, same-socket survival and no recipient custody |
| `sender-settlement` | The same entry; Query/Settle through Registry to recipient Mailbox, existing custody replay, unknown tombstone and later Submit refusal |
| `sender-retirement` | The same entry; authenticated sender fencing, original CallId receipt replay and one recipient control effect |
| `http-body-keepalive` | The same entry; exactly 2 MiB unauthorized production enrollment with a withheld suffix, then valid enrollment on the same physical TCP socket |
| `http-body-over-limit` | The same entry; eight route/auth/method variants with 2 MiB+1, each requiring 413 followed by physical EOF/reset; one grouped scenario |
| `creation-gates` | The same entry; normal enrolled sessions, four creation-gate outcomes, same-attempt replay/reconnect/retry, first Copy piece and held Claim refusal; one grouped scenario |
| `delivery-config` | The same entry with an explicit UUIDv7 relay after the mode; first/replay/update/completed readmission/replay return that relay; one grouped scenario |
| `delivery-config-refusals` | The same entry without a relay argument; actual Worker missing relay field and malformed peer refusals; two groups |
| `mailbox-deadline` | The same entry; normal session, offline recipient, actual alarm, newer status and released payload; one group, no physical restart claim |
| `installation-directory` | The same entry; multi-page normal-session ordering/cursor refusals and immutable revoked bindings with Submit refusal; three groups |
| `fleet-notices` | The same entry; three admitted sessions, two reader installations, journaled Fleet hints through the real alarm/Delivery path and exact positioned notice readback; one grouped scenario |
| `copy-owner` | The same entry; actual owner HTTP Health before initialization, shell storage bytes, empty Page, Repair response, machine refusal and minimal retired receipt; one grouped scenario |
| `owner-registry` | The same entry; normal Fleet/Register, Page/Migrate, holder Release/fence/target renewal, native preview/tamper/takeover/replay and held-lease refusal; three groups |
| `owner-retirement` | The same entry; public import/unheld/seed fixture followed by normal owner retirement, exact Fleet acknowledgement and stable receipt/replay/Migrate; one group, plus the driver's separate post-stop physical row readback |

All entries share one `WorkerdProcess` wrapper and own a JWKS fixture. The wrapper
waits on workerd's Ready event, uses isolated persistence, stops its own process between
restart phases and reaps it. The runner requires the exact selected case marker
and owned-process settlement, including two settlements for transaction/numeric,
Step and Bootstrap reopen scenarios. These supplement the subprocess exit and
do not replace reading the assertions and durable readbacks.

For current Access verifier controls, explicitly select `access-carryover`:
zero-age JWKS refresh, RSA key profile, verifier controls and same-token expiry.
Its four groups are not the retired workflow's outer/inner log-layer proof.
`access-sessions` separately covers normal signed HTTP/socket admission and
revocation. Each selection uses fresh persistence and must stop/reap its process.

The three sender modes each run one shared native/Worker scenario through actual
admitted sockets and recipient custody. They do not establish positive Worker
forwarding, forwarding races/reopen, normal registration, or full session/holder
equivalence. Select them explicitly; the default modes remain unchanged.

For the seven session and five holder obligations, explicitly select
`session-capabilities`, `session-failures` and `holder-lifecycle`. Decisions 604,
616 and 620 govern their source oracles; the permanent examples are
`normal_capability_cases.rs`, `normal_session_cases.rs` and
`normal_holder_cases.rs` under `examples/session_scope_workerd/` in the Tokio
crate. All start with actual enrollment, authenticated upgrade and wire Hello.
Count the capability two plus failure five as seven session groups, and the
holder five once. `holder-schema-overlap` selects an already included case in
the product example and is deliberately not another catalog journey.

The fifth holder group calls the existing fresh-hub SessionKey primitive only
after normal admission on its owning Connection. Separately, a changed Hello
SessionId on that admitted wire must return typed `MalformedFrame` and physically
close. It is not a wire answer that differs solely in SessionId. Separate real
installations exercise the healthy-versus-failed attachment case. The former
SqlTx pointer identity is retired; capability checks now observe actual
workers-rs attachment encoding/origin and the same object's real alarm.

The original ten closure selectors contain eleven groups, since `imported-claim`
contains two. Decisions 645/650 and the declared leaf handoffs govern them.
`ConformanceBoardState` selects Open or Archived only in the existing real
Fleet Import/Registry ImportTask/Copy seed arranger. It is not HTTP import proof
or a production admission policy. Open retirement refusal requires the exact
GateRefused answer and unchanged readable seed; successful Claim requires exact
epoch/start/expiry, replay and actual published holder/Copy fence. Current
Retiring/Retired state must take the specified precedence over cached answers.
The separate `claim-archived` selector covers Claim on an Archived, unretired
board (decisions 667/669/672). It requires exactly `board_archived`, with the
owning Fleet reservation `None` before and after the normal request. The existing
owner-only conformance Fleet receipt control reads through the production
decoder; it adds no public route. Its shared oracle also runs in the native
`archived_claim` integration test. Require both admission and premature-reservation
wrong builds to fail: an exact refusal alone cannot establish absence of a row.
This selector adds one group beyond the ten closure selectors' eleven groups.

Forwarding setup normally enrolls both peers, creates the Fleet graph and
Registers the holder. Shared test bodies keep Native and Worker literal oracles
in one home; the holder is a fixture socket, not the Router writer. Capacity
holds the real sixteenth Registry dispatch, observes the seventeenth refusal
before releasing it, and verifies terminal slot reuse. Deadline uses the owning
sender clock and actual alarm, then rejects the late answer on a fresh identity.

`forward-reopen` stops/reaps the actual Node/Miniflare owner, scans only its
stopped private persistence for a known original sender identity (presence
control) and absence of the opaque-post marker, then starts another owner on
the same root. Inspect the logged started/reaped PIDs to establish distinct
processes in addition to the required two reap markers. Existing sixteen slots
must remain occupied before original expiry; after it, sixteen fresh unanswered
deliveries and exact seventeenth Overloaded establish public capacity reuse.
This is not a private zero-row read. Qualified Miniflare's requested SIGINT may
exit 130; the harness accepts only 0 or 130 and records it. Do not claim graceful
disposal, workerd-child PID capture, eviction, hibernation, hosted behavior,
whole legacy-row equivalence or real Router writer/outbox closure from this run.

Rollback arms a one-shot fault after the real callback writes. The public journal
must retain the real report at position 0 and no attach receipt at position 1;
no Welcome may precede the ordered one-shot socket barrier. Both callback and
actual SQLx-overflow cases must then retry successfully. The barrier and journal
observer are `conformance-faults` surfaces, not normal protocol. Production
exclusion needs the separate `worker-release` guard and artifact scan. These
local groups do not establish eviction/hibernation, hosted/staging behavior,
full move, HTTP import, Router outbox recovery or whole-PR readiness.

The HTTP body modes exercise production routes in the local dev conformance
image. Keep-alive requires the inclusive limit, no early refusal, socket reuse
and no unauthorized peer reservation. Over-limit observes physical closure;
workerd strips the hop-by-hop `Connection` header. Its eight iterations include
two POST-enroll variants with different auth but identical printed route text.
Count the grouped result once; printed lines are not distinct-case identities.

`creation-gates` uses actual Fleet/Registry/Copy owners. It checks the recorded
carrier against the enrolled writer, exact rejection detail, and a retryable
missing creation followed by the same attempt succeeding after reconnect. Its
four gates, first-piece and held-Claim markers are subcases of one result. The
owning clock advances to drive retry work. Positive imported Claim, physical
server/writer startup recovery and Router writer/outbox startup remain separate
proof; this group establishes no legacy-row equivalence.

For decision 524's configured relay boundary, select `--workerd-mode delivery-config`
and supply `--delivery-relay-peer "$RELAY_PEER"`. There is no apparatus default:
the value must be UUIDv7 and differ from the scenario's configured receiver. The
driver passes it after the mode to `session_scope_workerd`. Use a fresh evidence
directory and persistence for each configured value when comparing two values.
This mode compares complete normal enrollment responses and uses the owning
revocation/readmission journal's operation IDs and `Completed` state before the
readmission checks. The original five stage markers and the extension below
remain subcases of one grouped result.
It does not exercise missing/invalid Worker bindings, Router Submit/Fetch/Received
or wrong-relay transport refusal, physical restart, or decision 539's peer refresh.
Central returns the relay; decision 524's amendment puts the delivery-call relay
check in the Router transport, since the session wire has no relay field.

Decision 909 adds a second lifecycle in this same scenario, with a distinct
installation, peer, Access client and fixture credentials. After that peer's
referenced revocation is Completed, arm the existing DropNextObjectReply for
Mailbox(installation), then readmit. Await the real dropped reply concurrently
with the unchanged ten-second readmission Completed oracle. The drop must name
central.open-peer-mailbox, that exact target peer and the unit request body;
the lifecycle must match the enrollment row's operation ID and revoked=false.
Require exact full enrollment identities and replay after both waits complete.

Select the same scenario natively with --package collaboration-central-tokio,
--features conformance and --test-target delivery_config. Require the five
original stages, five lost-reply stages and the explicit successful Open-loss
assertion on both runtimes. These remain one Native case or one Worker group,
not eleven distinct cases. The earlier unexplained Worker timeout stays open;
a passing lost-reply case is coverage, not a diagnosis or production repair.

In private Worker source, suppress alarms only after the actual reply-drop
observation. The original delivered-reply stages must still pass; the new case
must observe its exact drop, then fail the original ten-second readiness check
with the referenced readmission Running and Open CallPending/MayHaveReached.
A failure before the fault is armed supplies no sensitivity credit: retain it
and its disposition. Keep owned Node settlement, exact source/image restoration
and a fresh-state restored pass. No production hook or longer deadline follows.

`fleet-notices` covers decision 497's normal producer path. Two installations
read Fleet at their applied cursors, a third mutates it, and each reader receives
and reads back machine notices 1 and 2 for the exact committed prefixes. Replaying
the mutation must preserve the decision and leave one notice for that event.
The task is admitted with the mutation; Fleet's own alarm drives Delivery.
Decision 497 replaces the older PD3 custom-outbox wording with the durable-steps
journal. Reader progress is the maximum valid request `after`, never the offered
page's `through`; native `fleet_notice_outbox` cases separately exercise that
rule, transaction rollback, dispatch/ACK loss, owning store/runtime reopen and
archive-holder notices. Those component cases use core-envelope fixtures and
explicit alarms. This Worker mode proves neither fault/reopen parity nor a real
Registry producer of archive projections, physical/automatic server restart,
or full legacy rows 27–28 equivalence. Run with fresh persistence and require
the complete `FLEET_NOTICES_RESULT` group and owned process settlement.

`copy-owner` covers decision 473's bounded operations surface, as bound by
decisions 566, 570 (including its correction) and 572. It checks Health before
Copy initialization, the receiver's `stage` and status, the target shell's
positive storage measurement with no caller override, owner-only Health/Page/
Repair, page-limit refusal and the minimal retired Health and Repair responses.
Copy creation and retirement use the existing conformance fixture. Require the
complete `COPY_OWNER_RESULT` and one owned process settlement; its printed case
is a subcase of that single group.

This group does not seed pending DirectoryUpdate work or expired staged parts.
An empty-copy Repair response cannot distinguish an implementation that skips
those duties. The separate native `copy_receiver` target's
`owner_repair::owner_repair_discharges_real_directory_debt_and_expired_staged_parts`
enters the production owner envelope with both duties pending, observes the real
Fleet update and expiry, and requires a later Repair to make no second RPC.
Select `--test-target copy_receiver` with that exact `--test-name` under the native recipe;
the Worker `copy-owner` group alone does not prove those duties.
Native `copy_owner_health` separately covers actual
migration rollback diagnostics, failed diagnostic writes, independent metadata
reads, recovery and populated exclusive paging; `copy_receiver` checks local
metadata exclusion from the replicated catalog and collision rollback. These
are distinct from full Worker migration-fault/recovery/populated-page parity,
physical storage exhaustion, full Worker migration parity, normal
Registry administration and complete legacy row 16 equivalence. Worker storage
measurement here is positive and shell-owned, with no independent quantitative
byte attribution. The `bootstrap` journey separately retains fifteen schema/
enrollment groups across restart; its enrollment oracle requires all four fields
and the configured relay peer (524).

For Native owner Health through migration reopen (legacy rows 13–15; decisions
734/735/738), run the Native recipe with `--test-target copy_owner_health`.
The eight cases include these two exact selections for focused observations:

- `--test-name owner_health_precedes_initialization_uses_target_bytes_and_refuses_machine_access`
- `--test-name migration_reopen::pending_migration_keeps_health_readable_and_preview_recovers_with_original_rows`

Cold Health's first response must show `copy=Known(None)` and no migrated
bookkeeping; repeated reads ignore a caller-supplied byte count, machine access
is refused, and stopped readback finds no central migration tables. The combined
reopen case normally creates and Registers a project, applies a schema-and-row
Piece, then stops the server before saving/removing only central's own receiver
history stamp 3. Require actual MigrationPending version 3, owner Health 200 with
stored 2/supported 3, the exact stopped durable diagnostic and independently
measured db+wal bytes, plus Preview 503. Restore the exact corpus/version/checksum
only while stopped, then reopen: Preview 200 retains the original holder, fence 1
and prefix 1:2, Health is current at 3 with no failure, and a normally admitted
reader sees schema 1 and the UUIDv7-keyed row with values 7 and "keep".

Wrong cold classification, hidden pending diagnostics (or a Preview bypass), and
failure to recover after stamp restoration must fail their deciding assertions.
Keep the full target's count separate from focused repetitions and restore exact
source after private probes. This fixture writes only central-owned history at
rest; it uses no receiver-private SQL. Its boundary is server/store close and
reopen within one test process. OS-level SIGKILL, Worker parity, remaining legacy
rows and final restack/touched-package gates require separate proof.

`owner-registry` covers decisions 577/582/586 through actual enrollment and
Register, with unequal installation and peer identities. Move must reach the
admitted holder's Release before fencing/grant; the target renews afterward.
Confirmation returns the native preview unchanged, and the altered preview is
refused. Replays retain the exact epoch and position. The three printed groups
are not three more native tests. Native `owner_registry` separately checks a
preview made stale by a committed piece, distinct presence/renewal, and the
typed epoch, schema-support and concurrent-transfer refusals.

Migrate uses an exclusive ascending page of at most 25. Native
`copy_owner_health::migrate_cases` observes 28 actual directory entries, including
a failed first Copy followed by successful later Copies. `object_bootstrap::owner_migrate`
checks real starting versions, receiver/hook failures, recovery, corrupt history
and the unchanged retired receipt. Results name their corpus; unknown versions
stay null. Decision 580 retires preversioned adoption: an existing receiver with
no central history is refused. The CLI's separate contract cases check following
each successful cursor, continuing after Copy failures, stopping uncertain/HTTP
pages once, the last completed resume cursor, totals and nonzero failure exits.
Those cases do not exercise live owner sign-in or a whole CLI-to-central run.

For a held owner Fence racing the old holder's Piece (legacy row 11; decision
740 and its adopted map), run the Native recipe with
`--test-target owner_fence_ordering --test-target owner_registry`.
The declared checkpoint has three ordering cases plus twelve retained owner
Registry cases. The ordering selectors are:

- `held_old_piece_commits_before_the_owner_fence_without_partial_rows`
- `held_owner_fence_commits_before_the_old_piece_with_the_exact_cutoff`
- `both_held_calls_resume_before_either_answer_and_resolve_to_one_complete_order`

Require normal enrollment, Fleet creation, Register and schema 1. Verify the
Registry's held CopyFence database/transfer/target/epoch/expected prefix, and the
old session's entire held Piece and sender, against the same Copy. Both holds
must still be present together before release. The concurrent case releases
both before awaiting either answer; ordered cases alone do not establish that
overlap. The existing source dispatcher holds do not expose a Copy transaction
barrier or prove parallel SQLite transactions.

Piece-first requires Applied through 1:2 and PreviewStale, Copy fence 1 and old
holder, schema 1 and both exact UUIDv7-keyed rows. Fence-first requires grant at
epoch 2/start 1:1, StaleEpoch newer 2/cutoff 1:1, Copy fence 2/new holder/prefix
1:1 and empty schema-1 rows; the target must actually Renew. Concurrent release
may produce either complete order. Repeat the same owner operation, require its
identical terminal answer, and recheck Copy state and rows. This observes public
replay and visible epochs, without asserting an unused private allocation floor.

False Fence success on PreviewStale and an altered stale cutoff must each fail
after both correlated holds. Restore exact private source and rerun green.
Native HTTP/socket/store proof here leaves Worker parity, OS recovery, remaining
legacy rows and the final restack/touched-package gate separate.

`owner-retirement` uses the conformance-only `importedUnheld` bridge to arrange
canonical Fleet/public ImportTask/CopyAdminSeed state, then observes the normal
HTTP retirement. A normal reader first sees the seeded row, and afterward sees
Retired. Fleet must acknowledge the exact retirement before HTTP answers Retired;
replay and Migrate must leave both native retirement times unchanged. The marker
does not claim a physical row count. After checking the owned process settlement,
the driver opens only this run's fresh persistence read-only, requires exactly
one `owner_retirement_rows` table and zero rows, and retains its file/hash/count
in `workerd-owner-retirement-readback.json`. Missing/ambiguous tables or retained
rows fail the run. This reads a fixture-authored project table, never a private
receiver table. Native publication cases separately cover Retiring remaining
Retiring and a store reopen after library commit but before directory ACK;
explicit alarms do not prove automatic process-start recovery.

For row 16's Native B-owned retired-receipt portion (decisions 742/743/745),
use `--test-target copy_owner_health`, or the exact selection
`--test-name retired_reopen::retired_health_repair_and_migrate_keep_the_minimal_receipt_across_two_reopens`.
The declared full checkpoint has nine cases. Native arrangement uses the existing
canonical Fleet import and public ImportTask/seed before server start; the Worker
bootstrap HTTP route is not available on Native and is not an import adapter proof.

Pin only the Copy clock to `2030-03-17T17:46:40Z`, then perform actual owner
Retire over HTTP. Require the exact minimal Health receipt containing only the
stored retiredAt. After shutdown, change only central's receiver-history version
3 to 999, retaining every corpus, other version and checksum. On each of two
fresh owning servers, require that same Health receipt, Repair 410/Retired,
the nonempty Migrate page's actual Retired item, and unchanged Health afterward.
Stop each owner before reading the whole history and require 999 and every
checksum unchanged. Never write a receiver-private receipt or query private
receiver tables to arrange this proof.

Wrong Repair bootstrap admission, loss of Migrate's early Retired gate and a
Health timestamp sampled from the new clock must fail after actual retirement
and history999 arrangement. Restore exact private source and rerun green.
The boundary is server/store reopen in one test process. D's real HTTP import
410, the whole mixed legacy row, Worker parity and OS recovery remain separate.

Decision 589's production fixture exclusion is a separate image check when
commissioned: build with the documented production command, require the
production guard to exit 0, and scan JS/Wasm for `importedUnheld`,
`conformance_imported_unheld`, `/test/imported-unheld-fixture` and
`owner_retirement_rows`. A binary `rg -a -l -e ...` scan must exit 1 without
diagnostics (no matches); keep image hashes and the scan. Restore the dev image
before conformance scenarios. Neither owner mode proves D1b HTTP import, a new
physical process restart, the A restack, G3/OB8 or whole PR readiness.

### Retirements and owed proof

`workerd_lifecycle`, `native_smoke` and the legacy staging launcher retired in
the §14 cut (508); no retained mode silently aliases them. The old runner modes
`conformance`, `dag-edge`, `access-profile`, `access-zero-age` and `access-workflow`
fail with explicit retirement reasons. The legacy-shell default session and
holder controls, and G18's `panic_inside_transaction` hook retired under 513.
Select the three normal session/holder modes above for their declared
replacements; the retired selectors supply no passing credit. Panic rollback
and store usability use their separately commissioned current-boundary proof,
never the deleted hook. Every run records this catalog; selecting a retired
mode fails before building or launching workerd.

A commissioned pre-cutover comparison may run the same permanent scenario
against a retained, previously qualified Worker bundle. Record the harness source
commit and both bundle hashes separately, use fresh persistence, and require the
intended cache assertion failure. Do not rebuild that comparator or attribute its
runtime behavior to the newer harness commit. Then build and run the declared
target normally. A successful Worker cutover observation does not establish
native capability integration or full response parity across runtime applications.

## What the checks decide

| Adopted outcome | Existing route and deciding readback | Remaining obligation |
| --- | --- | --- |
| Commit and rollback | `composed-transactions`, `fleet-receipt`; real owning stores and reopen | G18 panic rollback and store usability are not established by the retired hook |
| Exact scalar transport | `composed-numeric`; literal i64 extremes, 2^53 and 2^53+1, REAL 1.0, snapshot and receipt replay after restart | NULL, text/blob, wide rowid, other boundaries and corrupt stored-value refusal are not inferred |
| Bootstrap and admission fixtures | `bootstrap`; real owner controls, schema/history and enrollment across restart | Normal routing/admission, Copy apply and mailbox delivery are separate proof |
| Access and status behavior | `access-sessions` and `access-carryover`; real RSA/WebCrypto, fixture JWKS and normal HTTP/sockets | Local crypto is not hosted Access or complete status/body parity |
| Clock, rollback, lost answer and dispatch hold | `runtime-controls`; current composed controls | Physical eviction, real IO failure and blocking fsync remain unobserved |
| Session and holder replacements | `session-capabilities` two, `session-failures` five and `holder-lifecycle` five; actual admission, journal and socket observations | The fifth holder group combines a post-admission primitive with a separate typed wire close; no eviction/hibernation or hosted parity follows |
| Caller refusal | Retired aggregate/control entries are recorded without credit (508/513) | Selected session/carryover modes do not substitute for undeclared DAG/caller-refusal replacements |
| Sender delivery controls | `raw-submit-refusal`, `sender-settlement`, `sender-retirement`; normal sockets and recipient mailbox readback | Private installation-peer lookup and exact Registry presence need separate observations; positive forwarding remains separate proof |
| Worker HTTP upload completion | `http-body-keepalive`, `http-body-over-limit`; actual production enrollment, physical TCP reuse and closure at the 2 MiB boundary | Local dev image, not hosted or worker-release proof; chunked uploads, abort behavior and memory peak are separate observations |
| Creation and registration | `creation-gates`; real enrolled sessions, stored Fleet decision, Registry carrier gate and first Copy piece | Positive imported Claim, physical server/writer recovery, Router startup and full legacy-row equivalence remain unobserved |
| Configured delivery relay | `delivery-config` with explicit `--delivery-relay-peer`; five normal enrollment answers and owning lifecycle journal completion | Missing/invalid deployment bindings, Router delivery-call closure and mismatch refusal, peer refresh and physical restart need separate observations |
| Fleet notice production | `fleet-notices`; actual Fleet mutation/replay, alarm/Delivery and two readers' exact numbered hints and notice readback | Worker faults/reopen, real Registry archive projections, physical/automatic server restart and full rows 27–28 equivalence remain separate proof |
| Owner Copy operations | `copy-owner`, plus native `copy_receiver::owner_repair`; owner HTTP observations and separate real Repair duty effects | Full Worker diagnostic/recovery/populated-page parity and row 16 equivalence remain separate proof |
| Owner Registry and retirement | `owner-registry`, `owner-retirement`; real normal owner actions and a separate physical fixture-table read after workerd stops | D1b HTTP import, new process restart, live sign-in/whole CLI path, A restack and broader readiness remain open |
| One core, one owner | No behavioral journey substitutes for OB8 | Per-target graphs, visibility, lint inheritance, checked-SQL freshness and compiler gate await B6 and independent review |

Check each offered test for the wrong behavior its assertion must reject, at
the real boundary it claims. A callback error rejected before any write cannot
prove rollback after a write. A restart that explicitly calls all alarms cannot
prove automatic wake recovery. Bring such gaps to the Implementer.

## Evidence and cleanup

`run.json` retains source/archive identity, runtime, purpose, command arguments,
exits, logs, actual test/example executable hashes and open gaps.
`test-counts.json` holds native counts; workerd also retains `tools.json` and
`worker-bundle.json`. `workerd-observations.json` identifies each selected case,
example, completion marker and reaped-process count. `apparatus-modules.json`
hashes both runner modules. Every build checks free space and stops below 45 GiB.
After each observation settles, retain hashes in receipts and remove its private
extracted source, source tar, executables, test binaries and Wasm bundles. Keep
reports, JSON indexes, logs, patches and receipts; no Rust build output stays in
private temporary storage. Check exact ownership and consumers before removal.
Repeated marker prints are retained as occurrences, never distinct-case credit;
the transaction scenario verifies twice during preparation and after process reopen.
Command-local debug information and incremental settings keep builds bounded.

The native tests and workerd lifecycle own their fixture cleanup. On a failed
run, inspect that exact harness's settlement and surviving process identities;
the wrapper never kills by name. Keep logs and failed identities. Do not open a
live Turso file externally: use owning APIs or settle, stop, then reopen.

After all owned consumers settle, keep evidence and clean only the task's build
residue. Before a Cargo clean, preview it and confirm the exact target has no
active users. From the worktree root, run
`cargo clean --profile dev --target-dir "$PWD/target"` after
the corresponding dry-run, then return the build slot to the Lead. Preserve
receipts, logs and qualified artifact hashes outside build output. Return each
outcome as supported, unsupported or unknown with its route and source identity.
A build failure is setup evidence, not a product
scenario failure. Missing or zero-test execution is never green.
