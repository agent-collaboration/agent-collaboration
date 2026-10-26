# Local Worker conformance obligations

This section retains the former wrapper’s Worker selectors, qualification evidence and coverage obligations. The wrapper retired in0.1.102; these selectors are historical, not current CLI flags. Use the retained Rust `composed_scope_workerd` and `session_scope_workerd` examples with CI’s pinned tools and explicit scenarios. Current TypeScript target declarations are in `verification/declaration.ts`. An obligation without a mapped retained case remains unavailable. See [central runtime](central-runtime.md) for current entries, native coverage and shared cleanup rules.

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

