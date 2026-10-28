# PR3 closure after decision 645

Source: clean feat/central-one-core 6e10aa3ba1e18c459486bf0a76578eeaf47d0191,
parent accepted ea81a3d6. The four verification paths in 6e10aa3b are preserved.
The r4 plan was read completely (381 lines), as were decisions 539/555/561/604,
645 and the current owned proof/removal record. The unchanged complete plan
record remains in the preceding normal-Worker handoff. This list is remaining
work, not acceptance or evidence that an old pending row has been closed.

| Remaining obligation | Classification | Decisive current evidence and next action |
|---|---|---|
| B2c / 669/672 Archived unretired Claim refusal and absent reservation | own proof complete; short paired observation pending | Shared normal Native1/0/0 and Worker1/0/0: None before, exact board_archived refusal and None after. Two actual core variants caught on both targets, exact restore; strict checks and actual production exclusion pass. No whole-row/PR or independent acceptance inferred. |
| B5 / 670 MailWaiting second-hop notice publication | needs A's final pin | After A adds BoardNotice::MailWaiting{stream}, actual mailbox item/control acceptance publishes the next-position notice to the peer through the library notice path. Prove exact next position, disconnected Welcome catch-up and caught no-publish build. Optional same-stream pending dedup; no new timer. |
| B5 / 539 central InstallationPeers paged read, multi-page directory refresh through real Router→core | needs A's final pin | Current board-sync-protocol and core have no InstallationPeers DTO/read; A owns DTO/local directory/send/refresh. 539/548 join the existing fleet-refresh cadence. Add central read and close multi-page interaction after A6/final pin; do not add a second DTO or timer. |
| B2a / 555 synchronous Register while the same operation is driven by an alarm | needs A's final pin | Current registry_lease_calls.rs:100–118 is start→drive_due→status. Pinned68394e0's Engine has no start_and_settle. At final pin adopt the operation-scoped API, preserve typed Admission/Execution mapping and prove the actual held concurrent-alarm request; no global lock. |
| B2c/B3 / 561 remaining Worker imported-Claim and replay precedence | can do now | Native directory_session/claim_routes.rs remains the reference. Declared650 leaf8916fb12 owns Worker positive imported Claim/replay/AlreadyHeld, Directory and Copy fence. The subsequent own two Worker selections cover cached Register/Claim Retiring/Retired precedence, with two effective incorrect builds; grouped independent observation/Lead acceptance remain pending. No D1b HTTP import, private publication-version or full legacy-row credit. |
| B3 retirement admission parity and remaining mixed owner assertions | can do now | Native owner_registry/retirement.rs remains the reference. Declared650 leaf8916fb12 observes Worker exact Open-board GateRefused/replay, Live Health and identical seeded SnapshotPage; the explicit Archived caller preserves accepted successful retirement. Grouped independent observation/Lead acceptance pending. No physical row-count/restart or full legacy-row inference; positive retirement is preserved credit only. |
| B3/476 positive forwarding/capacity/deadline/reopen on Worker | can do now | Retained Native target stays7/0/0 after three oracles/socket helpers are lifted once into private shared product-test modules. Own normal Worker origin/opaque reply, disconnected presence and verified reconnect selections each pass1/0/0 with three caught incorrect builds; grouped observation/acceptance pending. Subsequent own shared-oracle Worker capacity/deadline selections each pass1/0/0 with two caught incorrect builds. Subsequent own Worker process-reopen group observes actual persisted correlation/body absence and retained/expired public capacity, with three caught faults; independent/Lead disposition pending. Full row19/real Router consumer scope is not inferred. No private query/control/legacy setup is added. |
| B1/B2/B3/B4 live legacy cross-boundary obligations and row-equivalence inventory | can do now | r4 requires case→obligation→current proof before removal. Current removal table has historical pending statuses; accepted components do not close every full row. First concrete task: add the existing exact archive report scenario to the normal Worker entry (row1). Reconcile covered rows against actual raw receipts and retain remaining rows2/3/5/6/9–16/26/28/33 with their full named assertions. D-owned HTTP import/validation/feed-retention rows17/22–25 stay D's. |
| B6 per-target dependency/visibility/lint/freshness and public-safety preparation | can do now | Precheck the current source graph and reconcile the structural disposition/inventory. The final OB8 decision is still repeated on A's final pin; no source-scan test or parallel verifier recipe. Public-safety clearance/push remain Lead's. |
| Final A6 restack, B5/Host wiring reconciliation, G3 real-Router consumer closure (OB2/OB3/OB6/OB9), final OB8 | needs A's final pin | r4 G3 closes the stand-in on the integrated board stack. A6 is still in progress and the Lead explicitly deferred the restack; keep one owner for notice reactions and preserve 6e10aa3b. Do not treat the isolated central component tests as full Router/writer/outbox closure. |
| G4/central-staging/hosted and final independent PR review/readiness | needs A's final pin | These need the final integrated stack and the Lead's staging/review commissioning; hosted execution is not currently granted. G-pin remains conditional on PR C merge/tag authority. Preparation can proceed, execution cannot be inferred. |

Already discharged, not remaining: decision604/616/620's normal seven session
and five holder groups accepted645; local G18 accepted609; F's three native
process-restart scenarios accepted618 and included in275/0/1. F is not a claim
that every old physical-restart row or Worker hibernation/eviction passed.

First can-do-now execution: row1's normal Worker archive-report port, reusing
run_archive_report_with_clock and the existing per-object Fleet clock control.
It needs no final library API, new protocol, registration injection or fixture
state. Native reference is archive_reports.rs; literal outcomes and exact
board/workspace/feed/ledger assertions stay in that same shared body. First run
is characterization, not a pre-fix red; then an ordinary incorrect archive build
must fail the same permanent scenario. No broad/full gate without Lead's slot.

Lead01a1594d-0fd4 accepted the split and order, with one declared commit per
coherent group and an ordinary incorrect-build check, narrow runs only. The
555 API already exists at library0118f96 and A has pinned it; B waits only for
A6/final A restack, not for the library implementation. Archive Worker baseline
passes1/0/0 exit0 through the unchanged shared Native oracle. No pre-fix product
red was observed. The subsequent completed incorrect-build and restoration results are recorded
below; this paragraph retains the initial baseline chronology.

## Archive-report leaf: own completed observation

The new permanent selector `archive-reports` in `session_scope_workerd` calls
the unchanged `run_archive_report_with_clock` body through real normal Worker
sessions. `tests/support/archive_report_scenarios.rs` supplies only the current
owner-authenticated per-object Fleet clock control, with typed pinned-clock
readback; no new fixture, protocol, capability, dependency or SQL is introduced.

Baseline Worker1/0/0, exit0; then the actual core duplicate-report branch was
changed from AlreadyRecorded to Recorded. Its Worker build exited0 and the
unchanged scenario failed exit1: `actual Recorded; expected AlreadyRecorded`.
Exact core bytes were restored, the actual dev image rebuilt byte-identical to
baseline, and the Worker scenario passed1/0/0 again, exit0. All three owned
processes were reaped and the loopback port18093 is absent. No historical
pre-fix behavior red is claimed. Native reference archive_reports is reused
from accepted full275/0/1 at edcadfba; the four reference source files are
byte-identical, not an additional fresh Native run.

Focused example check and Tokio package all-target strictClippy exit0. Four
final command source maps remain current. The qualified dev bundle's Core/CF
inputs and tool hashes matched before baseline; final baseline/restored bundle
hashes match. No production source change survives the ordinary check. Current
production exclusion is reused with unchanged Core/CF inputs; a second
equivalent release build or whole workspace suite was not run.

This proposes closure of removal-map row1's Worker portion only. It does not
close reservation races, the full move workflow, other legacy rows, G3, hosted
or staging. Independent observation and the Lead's row acceptance are pending.
The separately accepted645/604 session-holder component is preserved.

Evidence:tmp/pr3-closure-645/{archive-runtime-summary.json,
archive-final-freshness.json,archive-bundle-qualification.json,
archive-duplicate-recorded.patch,archive-owner-restoration.json}, plus exact
expanded commands and raw archive-*.log/.json receipts.

Build-output disposition: target/debug is9.0G, of which the prior project clean
left8.0G dependency artifacts. This active B closure sequence immediately reuses
the example and library outputs for its next ready leaf, so they are held rather
than cleaning after each small proof. All actual Worker processes are reaped;
evidence and images are outside target. Revisit at the next slice/full-gate end.

## Decision 650: own Worker imported-Claim and Open-board retirement observation

Lead01a1595c-2aa7 authorizes only the existing conformance import/seed arranger's
typed ConformanceBoardState::{Archived, Open}. Existing retirement callers now
select Archived explicitly; new Claim/non-archived retirement selects Open.
Open maps to the existing FleetImport Active variant, whose existing rule creates
an Open board. No production policy, route, schema, dependency or library pin
changes. In particular the fleet_claim_step archived-board refusal is unchanged.

The permanent imported-claim selector has two distinct groups. First, normal
owner retirement returns the exact board_not_archived GateRefused reason and
replays it, Health remains Live, and the normal session reads the identical
actual seeded SnapshotPage before/after, including the one literal row. Second,
the normal session receives exactly epoch2, start1:1 and the configured four-hour
expiry from the fixed owning Registry clock; the same attempt replays that full
grant, a fresh attempt returns AlreadyHeld for the actual admitted peer, normal
Directory reaches the actual installation/expiry and CopyStatus has fence2/acked1:1.
No forced alarm, lease injection, unchecked SQL, mock storage or private version
readback stands in for those observations. The setup is conformance public
Fleet Import + real ImportTask + actual Copy seed, not D1b's admin HTTP import.

Chronology: the initial example compile caught an incorrect ImportedBoardState
variant name, corrected from Open to the existing Active→Open mapping. Corrected
check0; actual Worker baseline2/0/0 exit0. The actual Fleet retirement gate's
Open arm was deliberately admitted: build0, scenario exit1 at the exact refusal
assertion (Pending instead of GateRefused). Restored that source; the actual
Registry Claim answer was deliberately given epoch1: build0, scenario exit1
after the first group passes, at the literal epoch2 grant assertion. Restored
exact source bytes, rebuilt the actual dev image byte-identical to baseline,
and the unchanged scenario passes2/0/0 exit0. No historical pre-fix red.

The existing owner-retirement scenario also passes1/0/0 after its caller selects
Archived; this is preserved regression credit, not a newly completed obligation.
Core/Tokio native all-target conformance Clippy and CF Wasm all-target conformance
Clippy pass -D warnings, exit0. The actual documented production Worker build
and existing production marker guard pass0; independent scan additionally finds
no imported-fixture route, fixture table, literal seed row or fixture provenance
marker. Production Claim/retirement-gate sources remain byte-equal to HEAD.
Final dev restoration, formatter/diff checks and exact source-map comparison are
retained with the receipt. No full-suite or workspace gate is claimed.

This proposes only Worker positive imported Claim and non-archived retirement
component closure. Cached Claim/Register Retiring/Retired precedence is next.
The private Registry publication acknowledgement version, physical file row
count/restart, D1b HTTP import, full legacy row bodies, G3/final OB8, final A6 pin,
hosted/staging and whole-PR readiness remain unverified here. The Lead's
grouped independent observation and acceptance remain separate.

Evidence:tmp/pr3-closure-645/claim-*.{json,log,patch}; decisive
claim-runtime-summary.json, claim-owner-restoration.json,
claim-production-fixture-exclusion.json and final source/image freshness.

## Worker cached Register/Claim retirement precedence: own observation

The permanent claim-retiring and claim-retired selections each use a fresh
normal Worker root. Before the actual import, normal Claim caches NotFound.
After the explicit Archived public Fleet Import + real Registry ImportTask/Copy
seed, normal Register caches and replays exact NotWriter{current:None}. Actual
owner HTTP retire with a mismatched fence returns exact CopyMismatch fence1,
acked1:1 and leaves Retiring; matching fence/prefix completes real Retired.
Retiring and Retired both override Register's cached answer. Retiring preserves
old Claim NotFound, fresh Claim returns TransferInProgress with the exact
retirement request identity, and normal Directory shows Unheld. Retired overrides
both old and fresh Claim with Retired. No private publication-version read or
extra alarm/control stands in for those outcomes.

Both baselines pass1/0/0, exit0; no historical pre-fix red. Deliberately extending
the actual central Retired shortcut to Retiring compiles0, then the Retiring
case fails its cached-Claim assertion, exit1. A proposed Retired shortcut-removal
probe compiles0 and remains green1/0/0: no sensitivity credit. Reading pinned
68394e0 change_completion.rs:333–356 explains why: public holder_change_answer
independently checks Retired before the stored transfer. That breaks the probe's
inference, not the product contract; no guard deletion or policy change follows.
After exact source/image restoration, a deliberate actual central Retired
answer→NotFound compiles0 and is caught at the old-Claim assertion, exit1.
Exact source/image restore, and both final readable permanent cases pass1/0/0.
The final dev image matches the observed8916fb12 dev image byte-for-byte.

Only permanent Tokio product-test entry/count/helper/scenario files and B proof
documents change. Existing owner HTTP/Pending and Claim helpers become visible
only to their sibling test modules; bodies remain the same. There is no final
Core/CF/SQL/DTO/policy/dependency/pin or Verifier-apparatus change. Tokio package
all-target conformance Clippy -D warnings passes0. Production exclusion is reused
from the immediately preceding leaf, with actual identical Core/CF/manifest and
bundle inputs; no redundant release build or full suite is claimed. All seven
actual Worker children, including the ineffective probe, are reaped, and the
loopback port is absent. Final exact source/image/format receipts accompany the
component declaration.

Expected values come from the retained Native claim_routes retirement precedence
oracle and the public lease protocol, expressed as literal refusals and exact
request correlation. The effective Retiring control precedes a readability-only
rewrite/constant-PeerId hoist of the permanent test; the same literal expected
answer and failing assertion are retained, and both final cases run after exact
image restore. Raw source maps retain the actual chronology; no stale run is
presented as a new exact-source run. No fresh Native run is claimed.

This proposes the Worker cached-precedence component only. The independent
Verifier/Lead grouped decision, private publication acknowledgement, D1b HTTP
import, physical restart, full legacy rows, forwarding/G3/final OB8, A6/final pin,
hosted/staging and whole-PR readiness remain separate obligations. Continue the
next ready normal Worker forwarding leaf, narrow only.

Evidence:tmp/pr3-closure-645/claim-precedence-runtime-summary.json,
claim-precedence-bundle-qualification.json, claim-precedence-owner-restoration.json,
claim-retired-wrong-answer-restoration.json and exact claim-retiring-*/
claim-retired-* raw command/image/patch receipts. Ineffective removal probe and
its failed runner assertion are deliberately retained, not counted as a product
failure or a caught incorrect build.

## Normal Worker forwarding origin/presence/reconnect: own observation

The actual Native forwarding wire helpers and three original oracle bodies are
now private shared product-test modules, used by both Native and Worker. Native
start/arrange-holder and its explicit grant/admission setup limits remain. Worker
instead enrolls both peers normally, creates Workspace/Board/Project with normal
Fleet operations from the actual holder carrier, and uses normal Register.
No imported/grant fixture, new platform control, unchecked SQL or public API is
introduced. The test-only OutstandingForward holds the named sender/holder
FrameIds between actual disconnect/reconnect phases and stores no post body.
The same Welcome SessionId predicate from Native is retained; this is not a full
Welcome-database-list proof.

Three permanent Worker selectors: forward-origin compares the exact project,
original verified ActorOrigin, opaque request containing i64::MAX, exact sender
frame and opaque holder reply containing i64::MAX. forward-disconnected requires
exact correlation and HolderUnreachable with stored Some(last_seen), after the
actual holder socket closes. forward-reconnect sends a real original request,
closes that holder socket, authenticates the same holder on a fresh session and
answers the exact outstanding identity with the original opaque reply oracle.
The holder is a real authenticated fixture socket, not the real Router writer;
G3 consumer execution is still owed.

Chronology: first compile caught the private selector's missing Copy bookkeeping,
fixed; corrected check0. Fresh retained Native target7/0/0, then Worker baseline
three distinct1/0/0 cases. No historical product red. Actual core Registry route
machine-label corruption compiles0 and fails exact verified origin; actual
forward effect driver losing last_seen compiles0 and fails stored presence;
actual selected-holder predicate reversal compiles0 and fails the bounded
original-correlation response after actual reconnect. Every wrong case exits1,
with no success marker. Exact core source bytes restore, actual dev rebuild is
byte-identical to baseline and8916fb12, and the three Worker cases each pass1/0/0.
A reconnect error-context string was added before controls, preserving the same
predicate and bound; fresh final Native7/0/0 covers the current shared source.
The Native target includes its retained identity fixture and four other existing
cases; do not count it as seven new normal Worker cases.

Tokio package all-target conformance Clippy -D warnings0; formatter/diff/source
freshness receipts accompany the declaration. All nine actual Worker children
are reaped; port18093 absent. Three original core files match HEAD exactly.
331 Core/CF/manifest inputs and the dev image were qualified before baseline.
No Core/CF/SQL/dependency/pin/Verifier-apparatus change survives, so the actual
production exclusion from8916fb12 is reused on identical inputs. No redundant
production build, full suite or workspace gate is claimed.

This proposes only these three Worker forwarding component observations. Worker
sixteenth-slot reservation/capacity release, original deadline/late answer,
correlation/slot/deadline process reopen and opaque-body persistence remain next.
It does not close full row19, private publication-version proof, real Router
writer/outbox actions, final A6/539/555/G3/OB8, hosted/staging or PR readiness.
Grouped independent observation and Lead acceptance remain pending.

Evidence:tmp/pr3-closure-645/forward-runtime-summary.json,
forward-bundle-qualification.json, forward-owner-restoration.json and exact
forward-*.{json,log,patch} command/source/tool/image/negative/restoration receipts.

## Normal Worker forwarding capacity/deadline: own observation

The two original Native lifecycle oracles now live once in a private shared
product-test module, with the original post/holder-frame/answer/outcome helpers
and literal transient post/reply markers. Native control HTTP and the original
physical-reopen/private zero-pending SQL body remain in the Native test. Worker
uses the existing normal enrollment/Fleet-create/Register arranger and existing
owner-authenticated per-Connection runtime control route. No new API, control,
private SQL query, fixture grant, schema, dependency or pin is introduced.

forward-capacity leaves15 real requests outstanding, holds the actual sixteenth
sender→Registry dispatch, requires exact Overloaded for the seventeenth before
resuming that dispatch, then the sixteenth's exact terminal reply frees capacity
for another actual delivered/replied request. forward-deadline pins only the
existing owning sender clock, observes exact original Unanswered at the original
30-second deadline, sends a late holder answer, requires a distinct fresh holder
identity and exact fresh reply; old/duplicate correlation cannot substitute for
that fresh outcome. Original expected predicates, frame identities and opaque
literals are retained; no negative readiness sleep is introduced.

Check0; initial retained Native target7/0/0, with two obsolete imports removed
before quality; both Worker baselines1/0/0. No historical pre-fix behavior red.
Actual core capacity guard>=17 compiles0 and fails the seventeenth's required
Overloaded response while sixteenth Registry dispatch is held. Actual expiry
terminal changed to Overloaded compiles0 and fails exact original Unanswered.
Each wrong case exits1 without a success marker. The shared capacity oracle
releases only its own held barrier on a failed observation, preserving the
failure and avoiding a dangling test dispatch; that error-only cleanup was added
before controls. Exact core source bytes restore; actual dev image is identical
to baseline/8916fb12. Both restored Worker cases each1/0/0. Final current-source
retained Native target7/0/0; its physical reopen case/private SQL body remains.

Tokio all-target conformance Clippy -D warnings0; formatter/diff/current-source
receipts accompany declaration. All six actual Worker children are reaped,
port18093 absent. No Core/CF/SQL/policy/pin/apparatus change survives; prior actual
production build/exclusion is reused on identical Core/CF/manifest inputs and
the same observed dev image. No broad/full gate or redundant release build.

This proposes these two Worker lifecycle observations only. Worker physical
process reopen, persisted correlation/slots/deadlines and opaque-body absence
remain next. No full row19, actual Router writer/outbox/G3, final A6/539/555/OB8,
hosted/staging or PR readiness is inferred. Grouped independent observation and
Lead acceptance remain separate. For subsequent Worker expiry after reopen,
the intended public observation is sixteen fresh held requests at a fixed clock
then exact seventeenth Overloaded: the actual guard counts all pending rows, so
an old occupied slot would prevent one of the sixteen fresh deliveries. Native's
private zero-row query stays; no private Worker query or new marker is invented.
The Lead received that bounded route/limits in01a1598f-74fb; no acceptance yet.

Evidence:tmp/pr3-closure-645/forward-lifecycle-runtime-summary.json,
forward-lifecycle-owner-restoration.json, forward-lifecycle-* command/source/raw
receipts and forward-seventeenth-admitted/forward-expiry-overloaded patches,
image hashes, intended-failure and exact-restoration receipts.

Build-output check at this declaration:target/debug9.2G, against8.0G dependencies
left by the prior project clean. The next active Worker physical-reopen leaf
reuses these artifacts; hold them rather than cleaning after each small proof.
All actual consumers are settled and receipts stay outside target. Revisit at
the next slice/full-gate end; no production or other worktree process is touched.

## Actual owned Worker forwarding process reopen: own observation

Permanent forward-reopen uses the same actual normal registered-peer arranger,
factored once with unchanged enrollment/Fleet-create/Register and explicit socket
cleanup. It prepares16 real forwards, retaining sender/holder frame identities,
advances10 logical seconds before first stop, closes sockets and stops/reaps the
actual Node/Miniflare owner. Its stopped persistence scan rejects the existing
opaque-post marker and requires an independently known original sender request
identity. First corrected baseline observes36 files/2,943,592 bytes (the exact
raw metric for each run is in its log). A new owner starts on the same persistence
root with a different actual Node PID. New normal authentication/Hello/Welcome
then exact Overloaded demonstrate retained old capacity before original expiry.
At original30-second deadline, bounded real request/Overloaded observations wait
for the actual owning alarm, then16 unanswered fresh holder deliveries at the
fixed clock and exact seventeenth Overloaded demonstrate all available capacity.
Fresh IDs must differ from original/each other. Exact first fresh correlation/
opaque reply and another terminal-released usable slot remain real. This observes
public capacity under the current guard that counts all pending rows, not a
private SQL row read. Native's original private zero-pending body stays intact.

The test starts logical time10 seconds in the past, advances10 before stop, and
advances to original+30 only after both normal Welcomes. That preserves an elapsed
interval across restart without scheduling new Hello's absolute platform wake
in the future. No readiness sleep, manual engine drive, timer translation, new
control/API/query/marker or fixture grant is added. The existing harness reports
actual owned Node PID/status; these are Node harness identities, not separately
observed workerd-child PIDs. It accepts only0 or the qualified dependency's own
SIGINT130 path; the original reaped marker is unchanged. Actual runs report130,
so no graceful-exit0 or completed later script-await-dispose claim is made.
Port-free checking and normal same-root new-owner behavior remain mandatory.

Two explained preliminary failures are retained. Initial characterization passed
before strengthening elapsed time. My added zero-only stop gate then rejected
the dependency's normal requested SIGINT130 before scan/reopen; qualified
Miniflare89523–89551/133472 installs that exit hook before the later script
handler. Dropping the extra is not a product fix. The next future-pinned-clock
run upgraded the sender but timed out at Welcome: own stopped diagnostic journal
showed schema publication running attempt0/no drive lease, due at pinned+10,
about9.15s beyond the actual Hello invocation. Core Clock is overridden but the
actual AlarmOwner sets unshifted absolute DateTime; new Hello schedules a future
wake after SetTime's one immediate wall alarm. Starting time10s in the past fixes
that test setup while preserving the elapsed/original-deadline proof. The exact
5-second wire bound, real admission and all oracles remain. Debug artifacts:
tmp/debug-workflows/2026-10-20-codex-router-central-workerd-sigint/ and
2026-10-20-codex-router-central-workerd-clock/. These narrow read-only diagnostic
queries are not copied into the product or permanent scenario. No product or
library defect is claimed; no runtime/workaround change follows.

Current clock-aligned baseline1/0/0. Three actual core incorrect builds compile0,
then fail the intended permanent case: wrong stored request correlation fails
known-identity presence; deliberately persisting the actual opaque post in the
private correlation JSON fails raw body absence; original deadline extended30→40
fails expired public capacity after restart. Each exits1 without a success marker.
Wrong private serialization is used only on fresh synthetic fault state; no field
or format change survives. Exact operations/forward-store/session-forward bytes
restore to HEAD and actual dev image matches aligned baseline/8916fb12. Final
restored two-owner case1/0/0. The existing forward-origin case1/0/0 after factory
refactoring is regression credit only. No historical pre-fix product red.

Tokio/Conformance package all-target strict Clippy0; formatter/diff/current-source
receipts accompany declaration. Source-map collection additionally includes the
Conformance example/harness files consumed by this proof. All9 scenario attempts
and14 actual Node lifetimes (including the two explained failures and factory
regression) are retained separately; primary current baseline/3 controls/restore
use8 actual lifetimes for one distinct permanent group. Every recorded PID is
reaped; no summing repeats into new passing cases. Native target7/0/0 at1e87b502
is reused on unchanged Native target inputs, not rerun for a test-only factory/
Worker-process change. Core/CF/manifest production inputs are identical to prior
actual production exclusion; no redundant release/full gate is claimed.

This proposes the actual own-interrupt Worker process-reopen/correlation/body/
capacity/deadline component observation. DO eviction/hibernation, direct private
zero-row read, graceful-dispose completion, actual Router writer/outbox/G3, full
legacy row equivalence, final A6/539/555/OB8, hosted/staging and PR readiness remain
separate. The Lead owns grouped independent observation and acceptance.

Evidence:tmp/pr3-closure-645/forward-reopen-runtime-summary.json,
forward-reopen-owner-restoration.json, forward-reopen-* source/tool/command/image
receipts plus reopen-lost-original-correlation, reopen-persisted-opaque-post and
reopen-extended-original-deadline raw failures/patches. Every preliminary failure
is retained as a failed attempt, not a product red or passing/effective control.

## Source-only follow-ups after declared9b4058a5

Decisions669/672 authorize the Archived unretired Claim coverage correction,
with the existing Fleet receipt control extended only under conformance. Native
uses its existing real-store import/seed arrangers; Worker uses its existing
conformance bootstrap, both selecting ConformanceBoardState::Archived. One shared
normal-session/readback body, two owed incorrect builds on both runtimes. No
execution before the Verifier window returns. Decision670 remains final-pin-only.

B6 current OC-8 precheck identifies the retained local sqlx-turso* implementation
and collaboration-service dev-dependency as a separate source-copy obligation.
Normal-only Cargo reversal was insufficient; normal/build/dev graphs establish
the consumer. Evidence is in tmp/pr3-closure-645/structural-audit and ownership/
removal timing is with the Lead (01a159d3). No deletion or dependency redirect.

Lead677 (01a159d9) accepts the six declared preceding closure leaves within
their stated limits and returns the worktree window. Archived Claim669/672 is
the next narrow leaf: commit the source draft on apparatus dce990ff, then execute
both runtime cases and both incorrect builds on each, exact restore and quality/
production exclusion. No full suite. OC-8 removal belongs to A under673. Row28
adaptation676 retires only the old duplicate-count oracle; both physical loss
paths still owe whole-process proof with one notice per original CallId/step.

## Archived unretired Claim669/672: own completed narrow observation

Draft0c73b4df sits on Verifier apparatus dce990ff. One permanent shared scenario
runs through normal Native/Worker sessions with typed actual Fleet readback.
Each target passes1/0/0; Archived-admitted and reservation-written-before-refusal
variants compile and fail the intended assertions on each. Core restore is exact,
final Native/Worker green, Core/Tokio and Wasm CF strict Clippy0, fmt/Ruff0. One
initial duplicate_mod lint is retained; final test wrapper reuses the same
configuration module once and is rechecked. Actual production build/guard and
new-variant/six-marker exclusion pass; the observed dev image is restored exactly.
Four owned Node lifetimes reaped; this is one group, not summed positive reruns.
Paired observation/Lead disposition are pending; raw artifacts use archived-claim-
prefix under tmp/pr3-closure-645.

Next leaf676/678: both whole-process Fleet notice loss paths, one notice per saved
original call, positioned catch-up and unchanged records/feed/debt. Add the bound
post-success object reply loss pair on the existing conformance home only after
this target declaration; wait on its actual loss readiness, retain the no-op-drop
timeout control, and prove production exclusion of that new variant. No new
route/receiver-state injection/production behavior. OC-8 removal remains A's673;
MailWaiting670 remains after A's final pin.

## Decisions684/687–691: row26 before the parked row28

Lead684 accepts Archived Claim669/672 at2da6db32, closing667. Row28's real
Claim/Directory setup then exposed the absent Registry LeaseGranted producer;
its two loss/restart sources are parked intact at tmp/pr3-row28-parked-687.
687/689/690 bind the existing publication pass as the atomic producer, with
mark(database,epoch,holder installation), only Granted/Superseded, stable
(database,epoch,kind,installation) keys and no library API. A retires the dead
ReleaseRequested variant. Full row26 is declared separately before row28 resumes.

Own row26 process case has genuine pre-fix no-notice red, then positive literal
seven-day timestamps/position1/SIGKILL/restart/reconnect/automatic wall-clock
expiry.691 replaces the old Native persisted-platform-alarm assertion: current
library ensure_alarm is a no-op and startup reconstructs from persisted expiry.
The four compiled ordinary wrong builds all fail their intended assertions;
source restoration is exact. Native affected29/0/1 plus current bootstrap12/0/0,
strict Native/Wasm Clippy0. One bootstrap test literal7→8 is updated for the new
migration, outside the five binaries of the29 run. Actual Worker build/exclusion
and smoke still in progress; no paired/Lead row26 acceptance yet. No full suite
or workspace slot used. Row28 remains parked; its shared import fixture alone is
reused for row26 and the accepted Archived wrapper's oracle stays unchanged.

## Row26 declared own observation (687/689/690/691)

Own final affected Native41/0/1 passes on all final source, including full literal
Claim grant, position1, exact seven-day timestamps through SIGKILL/restart and
reconnect, and actual automatic prune before any notice read. Four meaningful
incorrect builds are caught and restored exactly; the expiry control is repeated
after strengthening the full grant oracle. Strict Native/Wasm checks, actual
Worker dev/release builds, production exclusion and restored image provenance
pass. Existing Worker imported-Claim2/0/0 is smoke only; new notice parity remains
Native-only. The immutable target/handback are row26-declaration.json and
 tmp/review-handoffs/2026-10-20-codex-router-central-registry-notices/.

Row26's paired observation/Lead disposition remains pending. No whole-row
acceptance is inferred. Row28's exact source is parked and its shared import /
reconnect helpers must be reconciled with row26 before resuming the two actual
loss/restart cases. No full suite or workspace slot was used.

## Row28 own candidate under676/678/699

On accepted row26 plus Verifier referencefd0f6159, the parked controls and both
physical Native cases are reconciled with the landed helpers. Final process
file6/0/1 passes, with exactly one append per original call and all kept identity,
payload, snapshot/feed/replay/debt/later-time checks. The stopped sender store
proves Running/exact input/digest before the new process begins. First fixture
failure used FleetRecords instead of the existing FleetSnapshot scope; corrected
without a product change. No-op drop and fresh per-Fleet-RPC receiver key each
compile and fail at the intended oracle, then restore exactly. All final source
maps current; strict checks and actual production/new-marker gates pass.

The candidate target/handback are row28-declaration.json and
 tmp/review-handoffs/2026-10-20-codex-router-central-fleet-notice-restart/.
Paired observation/Lead disposition remains pending. Worker smoke2/0/0 is ordinary
calls only, not active new-control or whole-process/DO-eviction proof. No full
suite/workspace gate. Final A pin, other legacy rows, G3 and hosted/review/PR-ready
work remain as named above. Execution window returns on declaration.

## Row28 accepted708; row33 own candidate709

Row28 is accepted by Lead708 after the paired observation. On its reference
commit de262d41, row33 is the next coherent independent leaf. Lead709 binds the
replacement for its old private pending-alarm assertion before test authorship.
New physical Native1/0/0 plus four kept component cases pass (five unique total),
both specified compiled controls fail at intended expiry timeout and restore
exactly; strict checks0. The initial compile diagnostic error earns no behavioral
red, and the passing characterization does not claim a pre-fix defect.

Actual runtime/guard sources are unchanged; old production marker evidence is
reused explicitly. No fresh Worker/new control, full suite/workspace gate, final
A pin or whole-PR acceptance. Row33 paired observation/Lead disposition remains
pending. Exact target/evidence are row33-declaration/runtime-summary/final-freshness
and the full row33 handoff. Rows2/3 source anchors exist without port/retirement
credit. Rows2/3/5/6/9–16, final A539/555/670/OC-8, G3/OB8, hosted/review/readiness
remain open. B returns its execution window on this declaration.

## Row33 accepted713; rows2/3 own candidate719

Lead713 accepts row33 and returns the window on76bc2c65.715/716/719 bind
the two physical rows and their exact proof substitutions before authorship.
Row2 preserves the actual new-run allocator/normal renewal/publication gate;
its delayed lower-fence/current-projection/Active-reservation segment explicitly
uses the stopped owning Fleet store and real ObjectRuntime. Row3 preserves both
dispatch and true ACK loss across SIGKILL, exact recovered call/input/attempt,
full finalized actor/origin/requestNone/time/feed, unchanged full ledger and
terminal no-repeat debt.719 retires only the old AlreadyHeld-producing fixture
and reason, replaced by actual Copy schema1/Hello max0 SchemaTooNew Refused.

Own affected Native15/0/1 (four new groups), six compiled intended failures with
exact restoration, strict Native/Wasm quality and Native/Worker actual image
inclusion/exclusion pass. Existing Worker smoke2/0/0 does not prove active new
handoff behavior or Worker restart. No pre-fix product defect is claimed; the
first gate timeout is uncredited. Detailed proof/chronology is in the trace and
rows23-runtime-summary/final-freshness/declaration plus the complete rows23
handoff. Paired observation and Lead disposition remain pending. No other row,
full suite/workspace, final A pin, G3/hosted or PR-ready claim. Remaining independent
rows5/6/9–16 continue only after the execution window returns.

## Row6 own candidate under728

Lead728 accepts rows2/3 on Verifier reference ee0130ce. Row6 own candidate
preserves both exact dropped answers, Welcome ack/lease, single schema/empty
snapshot and the original renewal attempt twice without expiry/lifetime extension
across SIGKILL/wait/new process. No assertion is retired; current Piece replay is
AlreadyApplied. Final full process target11/0/1, three current ordinary source
controls rejected after restart, strict checks0. First characterization is not a
pre-fix product red or an extra unique group.

Required validation exposed the retained row28 ACK-loss setup race. Existing719
atomic hold handoff now keeps actual lost-success debt pending through SIGKILL;
no clock change survives. A deliberately released next hold lets the actual
sender complete before death and the unchanged guard rejects that false recovery
setup. Exact sources/observations/restorations and the failed early-clock attempt
are retained. No production change, new mechanism, weakened oracle or new Worker
result. Detailed proof is in the current trace, row6 receipts and complete handoff.
Paired observation/Lead disposition pending; remaining independent rows5/9–16 and
final A/G3/hosted/review/PR-readiness remain open after window return.

## Owner rows 13–15 candidate under 733–735

### Owner rows 13–15: own candidate under decisions 733–735

Lead 733 accepts row 6 and its row 28 repair on Verifier reference 87af54fc and
returns B's window. Lead 734 binds the row 5 retirement mapping and the missing
row 13 Known(None) assertion. Lead 735 binds this native migration fixture.
The complete r4 record and delivery context remain unchanged. Library stays
68394e062e143a133d46943ef23d01095ab5fe7b. Only B product tests and B-owned proof
records change; no runtime, schema, manifest, pin, parent, apparatus or full
Verifier suffix changes.

Row 13 retains actual cold/repeated owner Health before migration, target db+wal
measurement, refusal of a caller-supplied measurement, machine denial and stopped
readback showing no central migration tables. It now explicitly requires cold
copy=Known(None). Its first characterization passes 1/0/0, exit 0. There is no
observed pre-fix product defect.

Combined rows 14/15 create real normal Fleet parents/project and Register, apply
an actual schema and values 7/keep through 1:2, and close the socket/server. The
fixture saves and removes only central's own receiver version 3 stamp at rest.
It does not edit private receiver DDL or data. On reopen the actual missing
migration meets existing DDL: Repair returns exact typed MigrationPending3;
Preview returns its existing 503 objectUnavailable. Health stays 200, with
stored 2/supported 3, independently measured target db+wal bytes and a diagnostic
that exactly matches central's stopped durable corpus/version/reason/time row.
The cause really contains 'already exists'. After restoring the exact saved
corpus/version/checksum and reopening, Preview returns 200 at the original
holder/fence 1/prefix 1:2. Health is current at 3 with no failure. A separate
normally admitted reader receives schema 1 and the exact one-row snapshot
[019f0000-0000-7000-8000-000000007735, 7, "keep"], with no next page.

The existing key contract requires UUIDv7 TEXT keys, so 7 is a value column.
The first runnable INTEGER PRIMARY KEY inference was refused by schemaUnsupported;
it is dropped, with the correction sent to Lead. Two compile attempts caught
OperationAnswer Eq/Debug assumptions; the first scripted Eq replacement missed
formatted code. Further setup runs caught invented typed Preview-error precision
and equality of two independently generated Preview tokens/current observations.
Current owner HTTP source and pinned preview_rules.rs show why those were invalid
agent inferences. The final oracle preserves exactly the 735 holder/fence/prefix
requirement and the full literal stored row. None of these failed fixtures earns
a product-red or negative-control claim. The corrected combined case passes
1/0/0, exit 0, with both phase markers.

All six ordinary wrong builds compile and run the actual selected case. Each is
rejected 0/1/0, exit 101: cold Copy unreadable; actual eager initialize before
Health; measured bytes incremented; current migration diagnostic hidden;
CopyRead allowed despite initialization failure (Preview wrongly 200); and a
sticky recorded failure after the exact stamp is restored. The last control
passes the complete pending phase before failing recovery. Each exact source,
patch, captured source map, intended assertion and byte restoration is retained.
All six differ from final captured sources only at their intended mutation.

Final full affected copy_owner_health target: 8/0/0, exit 0. This adds one unique
native combined case and strengthens one existing case. Strict native all-target
Core/Conformance/Tokio strict Clippy, formatter, scoped Ruff and whitespace
checks pass, exit 0. The first Clippy run rejected seven indexed row reads; exact
one-row slice matches retain cardinality checks. The final full target and all
six ordinary controls were rerun on those final bytes. Current receipts have
-checked/-quality-final/-format-final and control -final labels; earlier runs
remain chronology, not extra unique groups. Known workers-rs cache-template diagnostics remain in raw
Cargo output; no cache repair or relabeling of actual command statuses.
All own test servers, sockets and JWKS tasks are settled by the permanent tests.

The measured boundary is native server/store close and reopen in one test
process, as in the old owner cases. It is not OS SIGKILL recovery or DO eviction.
No extra manual repeat, whole-suite/workspace, new Worker/native production image,
final A pin, G3/OB8, hosted/staging or PR-ready claim. Runtime/manifests/guard and
actual image inputs remain identical to accepted rows 2/3; the existing image
observations are explicitly reused. All four observed dev-bundle hashes and
qualified tool hashes match; the final audit records that comparison.

Rows 5/9–12/16 and the final A539/555/670/OC-8, G3/hosted/review/readiness remain
open. The row 5 mapping is settled by 734, but no full row 5 assertion group is
closed by this owner leaf. Native/Worker claimed scopes remain distinct.
Paired observation and Lead disposition of rows 13–15 remain pending. B returns
its execution/build/test/generation/Worker window explicitly on declaration,
then waits on that real dependency with one saved self-wake.

Evidence: tmp/pr3-closure-645/owner-rows-{proof-map,runtime-summary,final-freshness,
precommit,declaration}.json; all first/compile/setup/current/restored/control logs;
owner-rows-control.py, owner-rows-audit.py and owner-rows-quality.py; this complete
canonical handoff. Tests bind 87af54fc plus the observed working bytes to the
later clean immutable declaration. No postcommit test run is invented.

## Gate policy received under 737


Decision 737 is received as the future restack gate: every touched package gets
its full all-target/no-fail-fast suite with CI features, plus actual Worker build
and journeys. Its request to identify never-run full suites was checked against
retained raw receipts: core/conformance/Tokio ran full all-target gates (250/0/0,
242/0/0 and later 275/0/1); the admin-protocol/core/Tokio full suite ran 277/0/0.
These are historical observations at their declared heads, not current or restack
proof. No newly identified never-run package arises in this test-only leaf. This
does not certify every shared package's historical coverage; the full touched-
package inventory and required repeated gate remain open at the final restack.
Exact receipt commands, heads and raw target counts: owner-rows-737-prior-gates.json.

## Row11 own candidate under739

Lead 739 accepts owner rows 13–15 at 69de26a0 on Verifier reference 5dbcdaf3
and explicitly returns B's window. The fired self-wake is finished, with no next
due. Its receipt is retained. The Verifier reference and complete suffix are now
protected relative to 5dbcdaf3. The unchanged complete r4 plan record/delivery
context remains verified; library 68394e0 is unchanged. Only B product tests and
B proof records change, with no new runtime, public seam, schema, pin or control.

Legacy row 11's complete old function (3ca2acea transfer_scenarios.rs:280 onward)
requires a correlated held Fence plus the exact old-holder Piece, both pending
before release. Legal outcomes are whole Piece commit plus PreviewStale, or a
whole Fence/grant plus exact stale cutoff and no rows from the refused Piece.
The new Native target uses real enrollment, normal Fleet parents/project creation,
Register and a normal schema1 Piece; no registration gate stand-in or private
receiver query. Installation and peer identities are unequal and remain distinct.

The existing object controls hold actual outbound Registry→Copy and
Connection→Copy requests before transmission. The Fence is checked against its
public call kind and full database/transfer/new epoch/expected prefix/target
holder. The session call is checked against kind, sender and the entire typed
Piece, including digest and prefix. Both exact destination holds must still be
present together before release. These are source dispatcher holds, not a new
Copy-internal transaction barrier. No helper or scheduler sleeps stand in for
operation readiness.

Three permanent cases cover the old obligation. The concurrent case releases
both held calls before awaiting either answer and permits exactly the two whole
legal outcomes. Separate deterministic release cases exercise each branch's
oracle: Piece-first returns Applied1:2/PreviewStale, Copy fence1/old holder/ack1:2
and both literal rows; Fence-first returns grant2/start1:1 and whole
StaleEpoch2/cutoff1:1, Copy fence2/new holder/ack1:1 and an empty schema1 snapshot.
The literal values (1,"overlap-1") and (2,"overlap-2") use distinct fixed UUIDv7
record keys under the current key contract, and the complete three-cell rows
are asserted. Fence-first's target can actually Renew the Registry grant.
Both branches repeat the same owner operation, require its exact stored answer,
and recheck identical Copy fence/prefix/schema/rows. No hidden unused Registry
allocation floor is claimed by those public observations.

Initial deterministic characterization was 2/0/0 exit 0, with one unused import
warning removed before controls. No pre-fix product defect was observed. A
self-check found that these ordered runs alone serialize too much to preserve
the old concurrent-release observation. That agent-added sufficiency inference
was dropped; the third run preserves concurrent release before either answer.
The initial complete three-case run passes 3/0/0, exit 0. No assertion was retired
and no public contract was changed. The earlier ordered-only receipts remain
chronology, not full old-row or extra unique-group credit.

Two actual incorrect product builds are each rejected 0/1/0 exit 101 after both
correlated holds: mapping real PreviewStale to false Fence success; and changing
the old Piece's actual stale cutoff from1:1 to1:0. A separate incorrect fixture
sends a valid different Piece; the concurrent case rejects it at the exact
correlation guard before any overlap-completion credit. All three final controls
compile/run and fail at their intended assertions, with exact source/patch/hash
restoration. The product controls retain two-hold readiness; the fixture guard
is separate evidence and is not counted as an overlap outcome or product defect.

Final affected Native target: ordering3/0/0 plus retained owner_registry12/0/0,
total15/0/0, exit0. Three new Native cases map one old-row obligation; retained
owner Registry cases are regression credit. Strict Core/Conformance/Tokio
all-target conformance Clippy -D warnings, fmt/Ruff/whitespace and final-source
freshness pass0. Known workers-rs cache-template parse diagnostics remain in raw
Cargo output; no cached upstream source was repaired or actual exit relabeled.
Every own socket/server/JWKS/HTTP caller is closed or cancelled and reaped; held
calls are released on error before teardown. No own compiler/runtime work remains
at declaration.

Native real HTTP/socket/store behavior is the proof boundary. No new Worker,
OS-process restart, DO eviction/hibernation, full package/workspace gate, final A
pin, real Router G3, hosted/staging or PR-ready claim. Runtime/manifests/guard and
actual image inputs remain identical to the accepted rows2/3 observations; all
four current dev-bundle files and qualified tools match their observed hashes.
That is explicitly reused image evidence, not a fresh run of this scenario on
Worker. The public owner operation is real; the client is the fixture socket,
not the actual production Router writer.

Decision737's full all-target/no-fail-fast/CI-feature gate for every touched
package and actual Worker build/journeys remains required at final restack.
The historical package receipts saved with the prior leaf are not current
whole-PR coverage. Row11 paired observation/Lead disposition remain pending;
rows5/9/10/12/16 and final A539/555/670/OC-8, G3/OB8, hosted/review/readiness stay
open. B returns its execution/build/test/generation/Worker window on immutable
declaration and saves one authorized self-wake for that real dependency.

Evidence: tmp/pr3-closure-645/row11-{proof-map,runtime-summary,final-freshness,
precommit,declaration}.json, first/overlap-first/final/raw control receipts;
row11-control.py and every wrong-source/patch/restore artifact. The final
-native-restored-final and -native-quality receipts bind5dbcdaf3 plus observed
working bytes to the later clean immutable declaration; no postcommit test runs
are invented. Verification apparatus and whole suffix remain unchanged relative
to5dbcdaf3. Only Lead DM is authorized; no board posting, direct Verifier message,
parent design/plan edit, install, push, deployment or production process operation.

## Row16 B-part candidate under742/743

Lead741 accepts row11 at937e78dd on Verifier reference801c64d1 and returns B's
window. The old self-wake is finished, with actual first fire and no next due.
The Verifier reference, apparatus and complete suffix are protected against
801c64d1. Complete unchanged r4 record/context and library68394e0 apply.
Lead742 binds row16's B-owned retired Health/Repair/Migrate portion;743 corrects
only the fixture's producer home for Native. D's HTTP import remains unverified.
No claim closes the entire mixed old row16.

The complete old local_copy_retirement body is read. It wrote a private legacy
minimal receipt alongside future bookkeeping999, then required exact retired
Health200, Repair410, MigrateRetired, unchanged receipt across server reopen and
D's import410. The current minimal Copy receipt intentionally has only retiredAt;
its old transfer/fence/acked receipt fields are dead contracts, as already settled.
The new B test preserves the retained observable obligations using actual current
native retirement and central-owned metadata, never a private receiver writer.

First Native compilation succeeds, but its Worker-style arrangement fails401
routeRefused before retirement or reopen proof. Source confirms the bootstrap
HTTP route is Cloudflare-only, and existing Native imported_board_fixture.rs
explicitly reuses canonical Fleet import plus public Registry ImportTask/Copy seed
before starting the server. The assumption that retire_imported_fixture's HTTP
producer existed unchanged on Native failed. The finding and proposed correction
went to Lead before dependent edits. Lead743 binds the existing Native fixture,
then the same Copy clock pin and actual owner Retire over HTTP. No new route,
alias, authentication bypass, fixture schema or HTTP import claim is introduced.
The first failed setup is retained as row16-native-first, not a product red.

The new permanent case arranges a real canonical Archived/unheld project before
starting Native, pins only the existing Copy clock to2030-03-17T17:46:40Z, and
completes actual owner Retire at fence1/prefix1:1. Initial owner Health must be
exactly {kind:retired,value:{retiredAt:2030-03-17T17:46:40Z}}; no legacy fields or
newly sampled clock value are accepted. The server shuts down before SQL.
Only central_schema_migrations' receiver version3 becomes999; every corpus,
other version and checksum must match the original history. No receiver-private
query/table or stored receipt is edited.

On each of two new owning servers, Health is the exact original minimal receipt,
Repair returns410 with exactly Retired, and the nonempty real owner Migrate
sweep returns one Retired result for the actual project. Health remains identical
after those operations. Each server is stopped and its entire central history
read independently; the999 stamp and all other row bytes remain unchanged.
The clocks reset with the new runtimes, so the stored literal receipt must survive
fresh clock observations. Actual server/store reopen in one test process is the
boundary; no OS SIGKILL or DO eviction claim.

Corrected Native characterization1/0/0 exit0 observes both reopen markers and
history readbacks. Three ordinary wrong runtime builds each compile/run and fail
0/1/0 exit101 after actual retirement/history999 arrangement: Repair admits
bootstrap despite Retired; Migrate loses its early Retired gate; owner Health
recomputes retiredAt from its current platform clock. The last fault passes the
initial pinned-time arrangement, then fails after reopen, so it challenges actual
retained-receipt semantics rather than only fixture initialization. All exact
wrong source/patch/maps, intended failures and byte restoration receipts are kept.
No pre-fix product defect is observed and no assertion is retired.

Final copy_owner_health target9/0/0 exit0 includes one new unique Native case and
eight retained cases. Strict native Core/Conformance/Tokio all-target conformance
Clippy -D warnings, fmt/Ruff/whitespace and source freshness pass0. Known workers-rs
cache-template parse diagnostics remain in raw output, separately from actual
successful Cargo exits; no upstream cache source was modified. All own stores,
servers, clients and local JWKS tasks settle; no compiler/runtime job remains at
declaration. Current source maps differ from each wrong build only at its named
mutation. Only B test files and B proof records change; no runtime, public seam,
production schema, manifest, pin, guard, parent or Verifier apparatus/suffix edit.

The existing Native import/seed fixture is arrangement through actual libraries,
not proof of D's HTTP import adapter or validation. The acted-on owner Retire,
Health, Repair and Migrate routes are real. No Worker parity, real Router G3,
new production image, full package/workspace gate, final A pin, hosted/staging or
PR-ready claim. Runtime/image inputs remain byte-identical to accepted rows2/3;
all four observed dev-bundle hashes and qualified tools still match. That is
explicitly reused image evidence, not new execution of this scenario on Worker.
Decision737's all-touched-package CI-feature gate remains due at final restack.

Evidence: tmp/pr3-closure-645/row16-{proof-map,runtime-summary,final-freshness,
precommit,declaration}.json, first failed setup/corrected Native/final/raw controls
and row16-control.py with every wrong-source/patch/restore receipt. Tests bind
801c64d1 plus observed working bytes to the later clean immutable declaration;
no postcommit test run is invented. Paired observation/Lead disposition remain
pending. B returns the execution/build/test/generation/Worker window on declaration
and saves one dependency self-wake before stopping. Ready remaining rows5/9/10/12,
D's mixed16 HTTP import, final A539/555/670/OC-8, G3/OB8, hosted/review/readiness
stay open. Only Lead DM is authorized; no board/direct Verifier message, helper,
parent edit, early pin, push, installation, deployment or production operation.

## Row5 renewal-subgroup candidate under746

Lead746 accepts row16's B-owned portion at7d55935c on Verifier reference5bdf2903
and explicitly returns B's window. The correct prior self-wake is finished;
its earlier malformed-path draft was cancelled before first fire/no dispatch.
No wake stays active during execution. The latest Verifier reference, apparatus
and complete suffix are protected relative to5bdf2903. The complete unchanged
r4 plan record/context and library68394e0 remain applicable.

This candidate covers only the complete renewals_and_supersession subgroup of
old row5 (3ca2acea session_scenarios.rs:203–287). Current lease_session_scenarios
checks a normal NotRegistered refusal, and accepted row6 preserves lost R1 over
physical restart, but neither supplies this full fresh-R2/after-takeover/expiry
sequence. Other old session groups and rows5/9/10/12 remain open; no whole-row
or cross-target equivalence is inferred from this subgroup.

Two permanent Native cases normally enroll unequal installation/peer identities,
create Fleet parents/project, Register epoch1 and commit the schema1/empty
Copy prefix1:1. The Registry clock is pinned to2030-03-17T17:46:40Z through the
existing object control; no clock, gate, writer, lease or storage stand-in replaces
the acted-on path. The credential remains valid, so lease expiry is not confused
with credential expiry. Reconnection closes the old socket and sends a fresh
ordinary authenticated Hello with the original credential, then checks the
exact typed session identity and the entire one-database Ready/LeaseStatus.
This does not claim a same-surviving-socket read or in-flight replacement proof.

The first case drops the exact outgoing committed R1 renewal answer, advances
the Registry clock15 minutes and reconnects. Welcome must preserve ack1:1,
held epoch1/original peer, original expiry T0+4h and remaining13,500,000ms.
The same R1 request replays=true, with that original recorded expiry and current
lease unchanged. A fresh R2 replays=false and expires at T0+4h+15m, with exactly
14,400,000ms remaining. A real owner Preview/native confirmation and Takeover
then grant epoch2/start1:1 to the other enrolled peer, and the owner operation
replays exactly. R1 afterward still returns its original successful epoch1/
T0+4h receipt, alongside current held epoch2/target peer/T0+4h+15m authority.
A fresh request from the old holder gets exact StaleEpoch2/cutoff1:1.
The actual schema1/empty snapshot stays at its original prefix.

The second case advances the Registry clock5 hours and reconnects normally.
Its exact Welcome retains original epoch1/holder/expiry T0+4h, now Expired with
zero remaining time. A fresh epoch1 renewal restores Held in the same epoch,
with recorded/current expiry T0+9h and exactly4h remaining; the same original
schema/prefix/empty snapshot stays unchanged. There is no takeover or new epoch
hidden in that expiration/restoration assertion.

Initial characterization2/0/0 exit0 found no pre-fix product defect. An explicit
clock-advanced marker is added before the expired Welcome for control attribution;
no oracle is changed. Three ordinary runtime wrong builds each compile/run and
fail0/1/0 exit101 at the intended layer: generate a new renewal request key on
every call (after original dropped R1); populate current authority from the
recorded result/calling peer (after actual owner takeover2); force Welcome state
Held even when its Registry lease is expired (after the five-hour clock marker).
Each wrong source, patch, captured source map, intended failure and exact byte
restoration receipt is retained. None is a test-only reported-error fabrication
or an observed baseline product red.

Final full new Native target2/0/0 exit0 covers two new cases and one complete
selected old subgroup. Strict native Core/Conformance/Tokio all-target conformance
Clippy -D warnings, fmt/Ruff/whitespace and final freshness pass0. Known workers-rs
cache-template parse diagnostics remain in raw output, separately from actual
successful command exits; no upstream cache source was changed. All own sockets,
servers and local JWKS tasks close and settle. No own compiler/runtime job remains
at declaration. Only the new B product test and B proof records change; no runtime,
public protocol, schema, dependency, pin, guard, parent or Verifier apparatus/suffix
change. Final source maps differ from each wrong build only at its named mutation.

The proof boundary is real Native owner HTTP/session websocket/core/store behavior
with the fixture client, not the real Router consumer. No OS restart, DO eviction/
hibernation, Worker parity, full package/workspace gate, final A pin, hosted/staging
or whole PR readiness is claimed. Runtime/manifests/guard/image inputs remain
identical to accepted rows2/3 observations; all four current dev-bundle files and
qualified tool hashes match. Those older image observations are explicitly reused,
not new execution of this scenario on Worker. Final737 package/CI-feature gate,
A539/555/670/OC-8, G3/OB8, hosted/review/readiness and D's actual mixed16 HTTP-import
proof stay separate.

Evidence: tmp/pr3-closure-645/renewal-history-{proof-map,runtime-summary,
final-freshness,precommit,declaration}.json, Native first/restored-final/quality/
format raw receipts and all three wrong-source/patch/restore records. Tests bind
5bdf2903 plus observed final working bytes to the later clean immutable declaration;
no postcommit test run is invented. Full canonical handoff and continuation prompt
are in central-renewal-history. Paired observation/Lead disposition are pending.
B returns its execution/build/test/generation/Worker window on declaration, saves
one dependency self-wake and stops for that real dependency. No board/direct
Verifier message, helper, early pin, parent edit, push, installation, deployment
or production process operation is authorized or performed.

## Row5 Move subgroup under750/751/752

Lead750 accepts the renewal subgroup at d1e24b90 on Verifier reference3c7714aa
and explicitly returns B's execution window. The prior wake is finished. The
Verifier reference/apparatus and complete suffix stay protected against3c7714aa.
The unchanged complete r4 context and library68394e0 apply. This next leaf covers
only move_refusals from old session_scenarios.rs:373–508 under734,751 and752;
other row5 groups and rows9/10/12 remain open.

Current tests normally enroll unequal installation/peer identities, create the
Fleet parent/project, Register epoch1 and apply schema1/prefix1:1. A real owner
Move sends a correlated ServerCall::Release. The clean path requires the second
Move's exact TransferInProgress, an actual renewal carrying the first transfer,
Release final1:2, exact Registry→Copy AwaitAcked payload/durable identity and a
real read-only-observed Copy waiter while Copy remains1:1. Only then does the
old holder apply the literal drained UUID-keyed row2. The operation must grant
and twice replay epoch2/start1:2, retain the exact Copy schema/fence/snapshot,
and allow the target's exact current epoch2 renewal.

Separate correlated holder answers preserve migration2 Pending and Failed as
exact owner refusals/replays, unchanged fence1/prefix1:1/empty schema1 snapshot,
and the old holder's original lease without an active transfer. The actual
holder is the fixture socket; this is central composition, not the Router's
writer implementation or proof of the migration's underlying local execution.

The first compile found an owned CallId move, corrected before runtime proof.
Initial runtime3/2/0 exposed an oracle/contract mismatch and a fixture-clock
issue, not established product defects. Existing board HolderUnreachable has
separate Renewal{lastRenewedAt} and Presence{lastSeen} observations;752 corrects
751's literal holderStale wire instruction. Keep the literal board response and
decode the actual response back to exact native HolderStale/HolderUnreachable.
No A DTO edit is made. The old +30-minute Unreachable assertion is retired only
as751/752 settle; stale renewal is now just beyond30min with exact original time.

The disconnected recent-holder case uses the existing read-only platform
attachment observer to confirm real HTTP socket removal before Move. Equal
pinned time retains the first Pending/NeverSent attempt. Source shows strict
now>firstAttempt+zeroBound and default initial retry10ms; a first1ms correction
was still below that due boundary and failed4/1/0. Advancing the actual Registry
clock10ms makes the same operation return exact native Unreachable through the
Presence wire shape. Its narrow case passes1/0/0. No artificial delivery failure,
private query, new control, schema, API, pin or runtime repair is introduced.
Final control and restored checks follow before declaration.

Final selected Native target5/0/0 exit0 contains five new unique cases for one
complete selected old Move subgroup. Four ordinary product wrong builds fail
at their intended layers: lost transfer notice0/1/0; bypassed actual Copy wait
0/1/0; changed migration number0/2/0; swapped Renewal/Presence observations3/2/0,
all exit101. Their six failing selections are sensitivity checks, not six extra
unique cases. The last build's three passing cases are repeated regression
observations. All four exact wrong sources, patches, intended phases, captured
source maps and byte-restoration receipts are retained. Final Native source maps
differ from each wrong build only at that build's intended product file.

Strict Native Core/Conformance/Tokio all-target conformance Clippy -D warnings,
fmt/Ruff/whitespace and final source freshness pass0. Known workers-rs cached
template parse diagnostics stay in raw Cargo output separately from actual
successful exit codes; no upstream cache code is changed. Every own HTTP caller,
socket/server/JWKS task and compiler job has ended. No baseline product defect,
new stand-in, runtime, protocol, schema, dependency, pin or guard change survives.
The fixture's release_unsent_for=0 and10ms engine retry boundary establish this
bounded Native delivery-failure selection; they do not prove a production policy
timeout. Copy wait/attachment observations reuse existing read-only controls.

The proof boundary is real Native HTTP/session/core/store composition with the
fixture holder. No new Worker, OS restart, DO eviction/hibernation, real Router
writer/migration execution, whole-row or whole-PR equivalence is claimed. The
hidden unused Registry allocation floor is not inferred from visible epoch2.
Runtime/manifests/guard/image inputs remain byte-identical to accepted rows2/3;
all four dev-bundle and qualified tool hashes still match. Those image/exclusion
observations are reused explicitly, not a fresh run of this scenario on Worker.

Whole rows5/9/10/12, final A539/555/670/OC-8, G3/OB8, all-touched-package CI-feature
gates737, D's mixed16 HTTP import, hosted/review/readiness remain open. Only734's
old unsolicited Release/gap/repeated-ack assertions and751/752's stale-Unreachable
meaning are retired as bound; no other old assertion is silently dropped. The
board HolderUnreachable wire shape keeps distinct exact Renewal/Presence fields
and actual reverse native decoding. Full row5 acceptance is not proposed here.

Evidence: tmp/pr3-closure-645/move-session-{proof-map,runtime-summary,final-freshness,
precommit,declaration}.json; first/corrected/mapped/retry-bound/restored-final,
quality/format and all four raw wrong-build/restore receipts. These runs bind
3c7714aa plus observed working bytes to the later immutable target; no postcommit
runs are invented. Full canonical record and continuation prompt are in the
central-session-move handoff. Paired observation and Lead disposition are pending.
B returns the execution/build/test/generation/Worker window on declaration and
saves one dependency self-wake. No board/direct Verifier message, helper, parent
edit, early pin, push, installation, deployment or production operation.

## Row5 registered in-flight replacement portion under757

Lead757 accepts the Move subgroup at1e13ef18 on Verifier reference261abfe9 and
explicitly returns B's window. The remaining follow-up wake is cancelled before
first fire, with no dispatched delivery. The latest Verifier reference, apparatus
and complete suffix stay protected relative to261abfe9. The complete unchanged
r4 context and library68394e0 apply. This candidate covers only the in-flight
replacement portion of old replaced_inflight_and_credentials, lines509–553 at
3ca2acea. Its credential portion and the other row5 groups/rows9/10/12 stay open.

The existing generic replacement test admits an empty session, so it does not
supply the registered held-renewal sequence. The permanent Native case reuses the
unchanged session_move/fixture.rs for normal enrollment, real Fleet creation,
Register1 and schema1/prefix1:1. It performs no Move and adds no fixture, runtime
or contract seam. The Registry clock advances15min. Its existing Connection
control holds the exact real Registry dispatch before delivery, and the oracle
checks the complete typed RenewRequest plus kind/source/face/destination/peer
and extracts its actual session identity.

A normal authenticated replacement reuses that exact session identity, expects
the whole one-database Welcome (schema prefix1:1, original held epoch1/peer,
original four-hour expiry and13,500,000ms remaining), then exact typed
ConnectionReplaced followed by physical close on the old socket. Resume clears
the owned dispatch hold. Same R1 is safely resent through the replacement. It
must settle exact recorded/current epoch1/holder, expiry T0+4h+15m, remaining4h,
transfer=None. Its first replayed flag is observed, not predetermined: Native
may cancel the original socket's request. The next same-R1 call must replay=true
with that exact settled receipt/current lease. The same replacement reads the
unchanged schema1/prefix1:1/empty snapshot with strict frame correlation.

The first draft added an original-call completion barrier after replacement;
it passed exact hold, Welcome and typed/physical close, then timed out waiting
for an original Registry reply. Native native_objects.rs:484–487 cancels and
drains old socket request futures; session_store.rs:281 awaits the effect driver
on that request. This was an agent-added extra, not an authorized survival
promise or a baseline product defect. It was dropped without a new mechanism.
The retained obligation is the old safe same-request resend, strengthened by
literal receipt and subsequent replay. Another owned fixture correction updates
Welcome's remaining time after the clock advance. Corrected Native1/0/0 exit0
observes first resend replayed=false, but that contingent flag is not made an
oracle or a general cancellation claim. The original call's committed fate is
not independently observed. The first failures remain chronology, not product
red-first proof or extra cases.

Two ordinary product wrong builds each compile/run and fail0/1/0 exit101 at
the intended stage: generating a new actual Registry renewal request key loses
exact settled replay after the replacement's first resend; changing the actual
Core outgoing replacement close reason to SessionEnded fails after the exact
new Welcome. A separate incorrect fixture sends another valid renewal request
under the hold and fails0/1/0 at full request attribution before the hold-credit
marker. The fixture guard is not a third product defect or extra unique case.
Every wrong source, patch, captured source map and exact restoration is retained.

Final Native1/0/0 exit0 covers one new permanent case and only the selected
in-flight replacement portion. Strict Core/Conformance/Tokio all-target conformance
Clippy -D warnings, fmt/Ruff/whitespace and current-source freshness pass0.
Known workers-rs cached template parse diagnostics remain in raw Cargo logs,
separate from actual successful exits; no upstream cache source is modified.
Own sockets, server/JWKS tasks and compiler work settle before declaration.
No runtime, public contract, production schema, dependency, pin, guard, parent
artifact or Verifier apparatus/whole-suffix change survives. The normal fixture
and all reused assertion helpers remain byte-identical to261abfe9.

The boundary is actual Native HTTP/websocket/core/store composition with a
fixture client, not the production Router consumer. No original-call completion,
Worker parity, OS restart, DO eviction/hibernation, full-row or whole-PR claim.
Current runtime/manifests/guard/image inputs match the accepted rows2/3 image
observations; all four dev-bundle and qualified tool hashes match. Those image
observations are reused explicitly, not new Worker/production execution.

Credential revocation/expiry/upgrade/audience assertions from old lines554–637
and other row5 groups/rows9/10/12 remain open. Final A539/555/670/OC-8, G3/OB8,
all-touched-package CI-feature gates737, D's mixed16 HTTP import and hosted/review/
readiness stay separate. No broad gate without Lead's serialized slot, early pin,
parent edit, board/direct Verifier message, helper, push, install, deployment,
tag or production process operation.

Evidence: tmp/pr3-closure-645/inflight-session-{proof-map,runtime-summary,
final-freshness,precommit,declaration}.json; first/safe-resend/corrected-Welcome/
restored-final, native-quality/format-final and all three wrong-build/restore
receipts. The first failures retain the dropped extra and clock-oracle correction;
no baseline product red is claimed. Runs bind261abfe9 plus observed final bytes
to the later immutable target, not invented postcommit execution. Full canonical
handoff is central-session-inflight. B returns execution/build/test/generation/
Worker window on declaration, keeps one authorized dependency self-wake, and
stops for actual paired observation/disposition. Full row5 equivalence and the
whole old replacement-and-credential function are not proposed here.

## Row5 credential portion under761

Lead761 accepts the in-flight replacement at30591911 on Verifier reference
c983b013 and explicitly returns B's window. Its wake fired and finished; the
follow-up intent prepared while the notice arrived was never submitted. No
wake is active during execution. The latest Verifier reference, apparatus and
complete suffix are protected againstc983b013. The unchanged complete r4 context
and library68394e0 apply. This leaf covers only old session_scenarios.rs:554–637.
Whole row5 acceptance is not inferred; rows9/10/12 and integrated gates remain.

Existing normal_access_scenarios proves typed revocation on an empty admitted
session and several Access assertion controls. machine_enrollment's stored-expiry
unit test changes a Registry row directly and does not prove live-frame expiry.
The new permanent session_credentials target reuses the unchanged normal
session_move/fixture.rs: unequal identities, real Fleet creation/Register1 and
schema1/prefix1:1. No new fixture schema, runtime control, protocol or private
query is introduced. Three fresh Native cases exercise the whole selected
credential portion through real owner HTTP, machine upgrade and session frames.

Revocation begins with an exact successful current-holder renewal, then normal
owner revoke must return literal MachineRevoked, followed by exact live-session
CredentialRejected(Revoked) and physical close. A new Access-signed upgrade with
the same stored machine credential must return401/Revoked. The separate target
socket reads the unchanged original Copy schema/prefix/empty snapshot.

Expiry also starts with an exact valid renewal. The reused ordinary enrollment
verifies its requested2033-05-18 expiry. Existing controls advance only Registry
and old Connection clocks to one millisecond beyond that actual expiry, and both
clock readbacks must match. A real Renew frame must receive exact
CredentialRejected(Expired) then physical close. A fresh signed Access assertion
is verified with the unchanged real edge wall clock, while Registry refuses the
stored machine credential401/Expired. The unrelated target socket reads the
unchanged Copy. This is machine-credential expiry, not Access JWT expiry, and
makes no universal thirty-day TTL or wall-clock passage claim.

The final case proves the actual registered holder works before and after the
refusals. A signed-machine Access assertion plus the old un-enrolled
machine-or-agent token string must return401/Unknown. Signed owner Access
cannot upgrade the machine route, and a valid machine credential cannot enter
owner takeover/preview: each must return literal wrongAudience and decode to
the exact AccessRefusal. The same original holder then renews and reads the
unchanged Copy. The un-enrolled string does not claim a real Router agent-token
producer or bearer audience bypass. Initial Native characterization3/0/0 exit0
found no baseline product defect. Literal audience oracles were strengthened
before controls; their normal expectation did not change.

The first control's insufficient source anchor matched two clock sites and
stopped before any source mutation or Cargo run; it is setup chronology, not
an incorrect-build result. The effective frame control selects the exact
on_frame forwarding/clock block. Other actual controls and final checks follow
before immutable declaration. No old assertion has been retired or weakened.

A subsequent final run caught an owned literal-oracle mistake introduced while
strengthening the audience checks: AccessRefusal is tagged {kind:wrongAudience},
not a bare string. The type's access_contract.rs:37–49 and actual401 body agree.
The intermediate restored-final run is2/1/0 exit101 and is not passing evidence.
Correcting that literal restores the exact existing contract, preserving typed
decode and literal equality. The corrected baseline passes3/0/0 before controls.
All six controls are rerun against those passing exact bytes, with unique
-corrected receipt labels. Earlier controls remain chronology; they are not
credited against the corrected final source or counted as extra unique cases.
The final passing Native receipt is native-restored-final-corrected, not the
older misleadingly named restored-final failure. No product behavior is changed.

Six ordinary product wrong builds each compile/run and fail0/1/0 exit101 at
its intended layer: reset the actual on_frame credential clock; bypass the
Registry upgrade expiry gate; bypass revoked-credential upgrade admission;
change the real outgoing Revoked close reason; misclassify an unknown stored
credential; bypass Access audience matching. The two upgrade bypasses actually
return101 after their live-session rejection stages, so those HTTP oracles are
not inferred from socket closure. All six exact source/patch/phase/hash maps and
byte-restoration receipts are retained. None is a baseline product defect,
assertion retirement, extra unique case or fabricated reported test failure.

Final Native3/0/0 exit0 covers three new permanent cases for the whole selected
credential portion554–637. Strict Core/Conformance/Tokio all-target conformance
Clippy -D warnings, fmt/Ruff/whitespace and current-source freshness pass0. Known
workers-rs cached template parse diagnostics remain in raw Cargo output apart
from actual successful command exits; upstream cache code is not edited. All
own clients, sockets, server/JWKS tasks and compiler work end before declaration.
No runtime, public protocol, schema, dependency, pin, guard, parent artifact or
Verifier apparatus/entire-suffix change survives. The reused normal fixture and
helpers stay byte-identical to c983b013. Final source maps differ from each
wrong build only at that build's intended product source file.

The boundary is actual Native HTTP/websocket/core/store with fixture credentials,
not the production Router or a real agent-token producer. No Worker parity,
OS-process restart, DO eviction/hibernation, full-row or whole-PR readiness is
claimed. Runtime/manifests/guard/image inputs and all four dev-bundle/qualified
tool hashes still match accepted rows2/3 observations. Those image/exclusion
observations are reused explicitly, not new production or Worker execution.

Whole row5 acceptance is left to the Lead's complete mapping; rows9/10/12,
final A539/555/670/OC-8, G3/OB8, full all-touched-package CI-feature gates737,
D's mixed16 HTTP import, hosted/staging and independent review/readiness remain
separate. No broad gate without Lead slot, early pin, parent/apparatus/suffix
edit, board/direct Verifier message, helper, push, install, deployment, tag,
default merge or production process operation is authorized or performed.

Evidence: tmp/pr3-closure-645/credential-session-{proof-map,runtime-summary,
final-freshness,precommit,declaration}.json; Native first/literal-corrected-baseline/restored-final-postreplacement,
native-quality-postreplacement/format-final-postreplacement and all six -postreplacement wrong-source/patch/restore receipts.
The ambiguous control setup stopped before source mutation or Cargo. Tests bind
c983b013 plus observed final bytes to the later clean immutable target; no
postcommit run is invented. Full canonical handoff is central-session-credentials.
Paired observation and Lead disposition remain pending. B returns execution/
build/test/generation/Worker window on declaration and keeps one authorized
dependency self-wake before stopping for that actual dependency.

The final old-source mapping also retained line553's precondition: revocation
acts on the replacement socket. The earlier fresh-connection case alone did not
establish that composition. The final case now captures the actual current
session ID through an exactly attributed normal renewal using the existing hold,
resumes and completes that renewal, then authenticates the same-session replacement
with its exact whole Ready Welcome. It requires typed/physical old close and a
successful current-holder renewal on the replacement before owner Revoke.
Revoked typed/physical close and refused reupgrade therefore apply to the real
replacement binding. No new public seam, private query or fixture-state writer
is added. The postreplacement baseline passes3/0/0. All six effective controls
are repeated on these final bytes with unique -postreplacement labels; each
revocation control must also reach the replacement-current marker. Prior passing
and incorrect-build receipts remain chronology, not additional unique cases or
proof against this final revision.

## Row5/9 shared nonempty lost-Piece group1 under768/769

Lead768 accepts the credential portion at e320a272 on Verifier reference
d8f75917 and explicitly returns B's window. Its self-wake fired and finished.
Full old dispatcher/body mapping in row5-residual-after-768.md identifies
additional branches; Lead769 keeps row5 open and commissions group1 first,
then the other transfer/DAG/unproven groups. Decision734 retires only old actor
cache eviction and unsolicited Release assertions. Same-socket read remains
transport evidence; physical persistence and DO eviction/hibernation are separate.
No wake is active during execution. Latest Verifier reference/apparatus and
complete suffix stay protected against d8f75917; library68394e0 is unchanged.

The whole old lost_piece_and_resume body (3ca2acea session_scenarios.rs:123–202)
and its producer were reread. Old row6's accepted physical restart loses an
empty schema Piece; it does not supply the literal nonempty row or the live
same-socket observation required here. Row9 invokes this same old subgroup,
so this leaf advances both maps without closing either whole row. The current
permanent session_lost_row target normally enrolls, creates the Fleet parents/
project, Registers epoch1 and applies schema1/prefix1:1. The legacy integer key
becomes a fixed valid UUIDv7 TEXT primary key; literal numeric1/label accepted
once are preserved alongside that exact key.

A real nonempty row Piece1:1→1:2 is sent over the admitted socket. The existing
exact-frame outgoing-answer fault must report that frame dropped. Without
close, new Hello, reopen, eviction or another connection, that same original
socket reads exact CopyStatus fence1/prefix1:2/schema1 and the complete one-row
SnapshotPage: fixed UUID key,1,accepted once, correct database/table/schema/
position and no next page. This is transport evidence, not runtime-eviction
or physical-durability proof. The test then honestly closes and authenticates
a fresh socket with the original session identity. The whole Ready Welcome
must retain prefix1:2 and the exact original Held epoch1/peer/four-hour expiry/
remaining4h. The identical Piece returns AlreadyApplied1:2 and the same socket
reads the exact row again. A second honest reconnect repeats Welcome, replay
and complete snapshot. No duplicate row, new epoch, altered payload or mixed
acknowledgement is accepted.

Execution was interrupted by the production Router maintenance the Lead reports
in01a15c49. The pre-cut lost-row-native-first log happens to show1/0/0, but no
run from that turn is credited per the Lead's directive. The old process ID is
unknown after restart and target/debug was absent at reentry. Role/commission,
trace scope/recent checkpoint, full retained test, residual map and769 were
reread; only that untracked product test was dirty. No stash/reset or production
operation was performed. The fresh post-reentry narrow build/Native run passes
1/0/0 exit0 from d8f75917 and the retained exact working bytes. The run took
85.65s rebuilding dependencies; its raw receipt is lost-row-native-reentry-first.
No baseline product defect has been observed. Recovery receipts and Lead notice
are retained separately; no former green output substitutes for this new run.

Three ordinary incorrect product builds each compile/run and fail0/1/0 exit101
at the intended layer: actual snapshot response drops the committed row, caught
on the original surviving socket after exact answer-loss readiness; Welcome
reports the old1:1 prefix, caught after the successful original-socket read;
real AlreadyApplied receipt is returned as fresh Applied, caught after the exact
reconnect Welcome. Each source, patch, raw log, stage attribution and before/
wrong/restored hash is retained. Every source restores byte-for-byte, and final
captured source maps differ from each wrong build only at its named product
source. These are three sensitivity checks for one permanent case, not extra
unique cases or baseline product defects.

Final Native1/0/0 exit0 is a fresh post-reentry run. Strict Native Core/
Conformance/Tokio all-target conformance Clippy -D warnings, fmt/Ruff/whitespace
and current-source freshness pass0. Known workers-rs cached template parse
diagnostics remain in raw Cargo logs separately from actual successful exits;
no cached upstream code is changed. Own socket/server/JWKS tasks and compiler
jobs settle before immutable declaration. Only this permanent B product test
and B proof records change. No runtime, protocol, schema, dependency, pin,
guard, parent artifact or Verifier apparatus/entire-suffix change survives.
The reused normal creation helper stays unchanged from d8f75917.

The proof boundary is Native HTTP/websocket/core/store with the fixture client.
The same-socket observations involve no new Hello or reopen and prove transport
continuity only. Actor-cache eviction is retired only as734 directs. Existing
physical owning-process proof remains separate; DO eviction/hibernation remains
unobserved. This does not close the full row12 redeploy fixture, other row5/9
branches, whole rows5/9/10/12, or real Router/Worker/hosted behavior. Current
runtime/manifests/guard/image inputs and four observed dev-bundle/qualified tool
hashes match accepted rows2/3 receipts; that evidence is explicitly reused,
not new Worker or production execution.

Decision769's next actual owner interrupted transfer, floor-repair/schema/
ordered groups, DAG and public missing-Copy/no-grant proof remain owed. The
private-floor corruption mechanism is library-owned and the missing-Copy literal
is adapted only as769 binds. Final A539/555/670/OC-8, G3/OB8, all-touched-package
CI-feature full737, D mixed16 HTTP import, hosted/staging, independent review
and readiness remain separate. No full gate was run or requested for this narrow
leaf; a later full gate still needs Lead's slot. Newly rebuilt output is held
for the active proof sequence and paired observation; no workspace cleanup after
every leaf, and no other task's artifacts or production process are touched.

Evidence: tmp/pr3-closure-645/lost-row-{reentry,recovery-authority,proof-map,
runtime-summary,final-freshness,precommit,declaration}.json, Native reentry-first/
restored-final/quality/format-final receipts and all three wrong-source/patch/
restore records. Pre-cut lost-row-native-first receives no current proof credit.
Runs bind d8f75917 plus exact observed final working bytes to the later immutable
target; no postcommit run is invented. Full unchanged r4 record and continuation
are in the central-session-lost-row handoff. Paired observation/Lead disposition
are pending. B returns execution/build/test/generation/Worker window on declaration,
saves one authorized dependency wake and stops for that actual dependency.
No board/direct Verifier message, helper, parent edit, early pin, push, install,
deploy, tag, default merge or production operation is authorized or performed.

## Row5 group2: stale-preview epoch oracle awaiting Lead disposition

Lead781 accepted b11b5e8f with Verifier reference61f49125 and returned B's
window. The earlier wake was cancelled before firing (no dispatch). On that
clean base, B wrote only the permanent uncommitted Native target
`owner_takeover_recovery.rs`; no runtime, manifest, pin, schema or control change.

The old complete `transfer_crash_and_preview` sequence at3ca2acea is retained
in tmp/pr3-closure-645/old-session-scenarios.rs:288–372. The new scenario uses
normal enrollment, Fleet creation, Register, schema1, the literal row2
"between preview and fence", actual owner Preview/Takeover and existing
conformance-only holds/drop controls. No product-state hook or private query.

Observed sequence so far: two compiler setup failures (wrong Piece-refusal
field names then wrong Option assumption; neither is behavior proof), followed
by actual Native0/1/0 exit101 after stale-preview refusal and exact Copy
fence1/prefix1:2/schema1/literal-row readback. One repeated diagnostic run made
no source change and adds no evidence. The final diagnostic captures the real
held Registry→Copy `fleet-sync.copy-fence` envelope: newEpoch3, original
preview fence1/acked1:2, target peer782 and schema support1. This fails the old
literal epoch2 oracle BEFORE any committed-reply loss or owning reopen.

The source cause is the pinned library's change_checks.rs:277–345: it reserves
and saves the next floor in the local check before the external Fence. The
valid-but-outdated saved Preview reaches Copy, which returns PreviewStale;
Copy's fence remains1, while the next fresh attempt reserves3. The real source
at holder_change_rules.rs:15–36/280–295 takes that reserved epoch. This is not
yet an established product defect or a red of interrupted-recovery behavior.
B does not silently weaken the old numeric oracle or change floor semantics.

Report01a15c61-0083 delivered to Lead (peerMessageWritten only). Candidate:
retain the single original fixture and public stale-preview/row sequence,
bind exact subsequent epoch3/start1:2, retain same-Fence identity after reply
loss/reopen/replay, old-holder stale cutoff1:2 and new-holder literal row3.
Lead disposition is pending. Dependent implementation/checks stopped; no
wrong build, successful recovery, Clippy, full gate, Worker or acceptance claim.
The draft still expects2; its reopen/replay/old/new phases remain unexecuted.
No own Cargo/runtime job remains. Whole rows5/9/10/12 and all previously named
integration/final-pin/hosted obligations stay open. No stand-in is introduced.

Evidence: takeover-recovery-native-{first,baseline,baseline-compiled,
envelope-diagnostic,envelope-detail}.{json,log}; exact latter source hashes
plus takeover-recovery-oracle-blocker.json. Next action: read Lead's exact
reply to01a15c61, keep or cancel the single saved follow-up wake, and change
only the oracle it binds before running this permanent target again.

## Group2 oracle bound by Lead785; unchanged runtime passes

Decision785 (01a15c62-be25) retires only the old numeric2 expectation. Keep the
same database, public stale Preview and literal row sequence; bind exact3 and
start1:2 throughout Fence loss/reopen/replay/old/new writers. Require historical
absence of any grant2. The blocker entry above is superseded only by this oracle
disposition, not by a product repair. Wake01a15c62-2ba3 was cancelled before
firstFire, with no delivery. No new wake remains active while B executes.

The added grant_readback.rs is a test-local executor adapter for the existing
public fleet_sync_lease::epoch_holder API. It runs only while the owning server
is stopped, in an explicit read transaction on a read_only registry.db connection,
then rolls back and closes. Before reopen it requires literal original grant1,
no grant2 and no grant3. After recovery it requires exact original grant1 ended
at1:2, target grant3 at the pinned second time, and no grant2. All scenario
mutations and reads of Copy remain normal authenticated HTTP/websocket paths.
No new product/private state hook, SQL query, migration, Host stand-in, manifest,
pin or conformance control is introduced. The adapter does not open or migrate
a store, dispatch a call or execute any product write. SQLx metadata is unchanged.

The first approved complete Native case, takeover-recovery-native-785-first,
passed1/0/0 exit0 on unchanged61f49125/68394e0 runtime. It loses the actual
successful original Fence reply, observes committed Copy3/row2, shuts down the
owning server, reads history, creates a fresh owning server, retries the exact
full original Fence envelope and CallId, grants/replays exactly3/start1:2, then
requires old-holder StaleEpoch3/cutoff1:2 and actual new-holder Piece3:1. Both
sockets read exactly row2 "between preview and fence" plus row3 "new holder";
"rejected tail" never appears. Final stopped history is exact and grant2 absent.

No pre-fix interrupted-recovery product defect was observed; initial recovery
behavior was green. Numeric2 was the authorized retirement, not a fixed defect.
Wrong builds will establish sensitivity, then restored green and strict narrow
gates bind the final leaf. Physical OS restart, DO/Worker, whole rows5/9/10/12,
Router/final A/integration/hosted/readiness remain outside this claim. Lead was
notified01a15c66-a40e. This entry is an in-progress checkpoint, not acceptance.

## Group2 candidate: restored proof and bounded handback under785

The restored final Native target passes1/0/0 exit0. Its first complete approved
runtime run also passed1/0/0 before any product mutation, so no pre-fix recovery
defect or red/green product fix is claimed. Four ordinary Core wrong builds
compile and execute this exact permanent test and each fail0/1/0 exit101:
- report a successful epoch3 Fence without applying it: the post-drop Copy
  holder/fence/prefix oracle fails before any owning reopen;
- return an incorrect epoch4 in the replayed Fence receipt: the exact original
  envelope has been retried after reopen, then public Takeover reports Failed
  rather than the required exact epoch3/start1:2 grant;
- map the actual old-writer StaleEpoch refusal to success: the old-holder
  exact whole refusal/cutoff oracle fails after the recovered exact grant;
- omit the last actual SnapshotPage row only at epoch3: the exact nonempty
  two-row snapshot oracle fails after recovered grant/new-holder Piece.
All four source edits were restored byte for byte. An additional ordinary
forgotten-Fence-receipt control also reaches the exact reopened call and fails
0/1/0 at a bounded Elapsed error. It missed its planned literal grant assertion,
so it is retained and excluded from the primary four. Do not report an extra
allocated epoch or a defective normal floor-repair path from that experiment.

Strict all-target conformance Clippy for Core/Conformance/Tokio (-D warnings),
workspace formatting check, Ruff for this leaf's retained evidence scripts,
whitespace and source-freshness audits pass with exit0. These are narrow native
leaf gates; no full workspace/full-suite/Worker generation run was performed.
The build logs retain the known workers-rs cache template-Cargo diagnostics;
actual Cargo exit/counts, compiler completion and test markers govern each run.
No dependency cache edit or skipped proof gate is concealed. Target/debug3.2GiB
and ample free disk do not justify a cleanup between this run and observation.

The two new permanent test files contain all scenario and history assertions.
There is no product runtime, schema, dependency, manifest, lock, pin, guard or
verification apparatus change. The existing dev bundle's four hashes and all
qualified tool hashes still match accepted rows2/3; prior native production and
Worker build/guard/smoke evidence is explicitly reused at identical inputs,
not a fresh platform claim. Verifier-owned apparatus and the entire observations
suffix remain exact against61f49125. No private library query is introduced.

This advances only shared row5/9 group2. Group1 b11b5e8f was accepted781.
Whole rows5/9/10/12 remain open. Remaining769 floor-repair/schema/ordered transfer,
actual DAG envelope and public missing-Copy/no-grant groups remain owed; the
private-floor corruption mechanism remains library-owned. Decision734 retires
only the deleted actor-cache operation/other exact named assertions;785 retires
only the old numeric2 after stale Preview. The native owning-server/store reopen
here is not OS SIGKILL recovery, Worker/DO eviction/hibernation, a whole redeploy
fixture, real production Router behavior or hosted/staging observation. Final
A539/555/670/OC-8, Router G3/OB8, all-touched-package CI-feature full737, D mixed16
HTTP import, independent review/readiness remain separate.

Evidence: tmp/pr3-closure-645/takeover-recovery-{decision785,proof-map,
runtime-summary,final-freshness,precommit,declaration}.json; first approved and
restored-final Native, native-quality/format-final receipts; all five wrong
sources/patches/restore records, with the extra timeout separately labelled.
Earlier compiler/setup/retired-oracle runs remain retained and excluded from
product-defect proof. Observed final working bytes and parent61f49125 bind to
the later immutable target; no postcommit test run is invented. Complete unchanged
canonical r4 and exact next actions are in the central-takeover-recovery handoff.

Paired observation/Lead acceptance of this candidate remain pending. B returns
the execution/build/test/generation/Worker window on immutable declaration,
keeps exactly one authorized dependency wake and stops for that actual dependency.
No board/direct Verifier message, helper, parent design/plan edit, early final
pin, push, install, deploy, tag, default merge or production operation occurred.

## Row5 group3 begins at accepted792 reference

Lead792 (01a15c76-c8a2) accepts3e950591 on Verifier referencefffae2d1 and
returns B's window. Own pending wake01a15c6f-9708 was cancelled before firstFire,
with no dispatched delivery. HEADfffae2d1 is clean before edits; library68394e0
and protected apparatus/suffix remain unchanged. The assignment is group3:
floor5→6 preview/Takeover/correlated Move, unknown schema support, holder schema
refusal and ordered rows. Preserve old producers/oracles first; report any
mismatch with source anchors before adapting it. No broad/full gate without slot.

Full old transfer_scenarios.rs and ordered_transfer_rows.rs at3ca2acea have
been read, including all before/after overlap branches. The overlap test is
accepted separately at741; this is not automatic credit for the distinct ordered
multirow values before-left/right, late-left/right and new-left/right. Existing
normal owner schema0 and epoch-ceiling cases are source references, not all
remaining group3 proof. Current ReleaseAnswer has a correlated server-call
ReleaseRefused(SchemaTooNew) path; no unsolicited old Release variant will be
invented. Existing owner conformance copy-call can arrange an actual public
CopyFence without private state edits; no new control is needed for floor5.

The first permanent uncommitted target transfer_schema_support.rs retains the
old known-max0 then never-enrolled-installation producer on one normal fixture.
It normally enrolls, creates Fleet parents/project, Registers1, applies schema1,
replaces target with actual Hello max0 and expects typed SchemaTooNew(Some0),
exact unchanged Copy1/prefix1:1/schema/empty snapshot. It then uses a fresh
never-enrolled UUID installation, exactly as the old unknown producer, and still
expects SchemaTooNew(None). Source PrepareOwnerTransfer checks target_peer
before task admission and returns NotFound for absent enrollment; that possible
oracle mismatch will be observed, not silently replaced by a different producer.
The narrow first old-oracle run is in progress; no green or group3 credit yet.
All product runtime/manifests/pin/schema/controls remain unchanged.

## Group3 absent-enrollment oracle corrected only by794

The first old known/unknown producer subset atfffae2d1 compiled and ran0/1/0
exit101. Actual Hello0 produced the exact SchemaTooNew(Some0) with unchanged
Copy1/schema1/prefix1:1. The never-enrolled UUID then produced exact NotFound,
not the old SchemaTooNew(None). Report01a15c7a-b257 carried the real log and
owner_registry_steps.rs:46 source before any oracle edit.

Lead794 (01a15c7b-1f70) binds the adaptation: retain that producer as its own
NotFound/no-grant/unchanged-Copy assertion and retire only its old label;
separately enroll a known installation without Hello/schema report to exercise
SchemaTooNew(None) at the real Fence. Known0 and correlated holder schema
refusal are separate permanent cases. All four use normal public enrollment,
Fleet creation, Register, actual session/owner requests and existing controls.
The shared test fixture arranges no grants with a stand-in. Public epoch_holder
readback on the stopped owning Registry asserts the exact original grant and
no grants2–6. No private SQL/hook/production control or runtime change.

The four-case test's first compile failed because its expected schema was moved
into the literal holder response; clone correction is test setup only and has
no behavioral-red credit. The corrected narrow run is in progress. No full
suite/workspace/Worker gate or whole group3 acceptance is claimed. Previous
wait was resolved by794 before a new wake was created; none is active now.

## Group3 first complete runtime result:9/0/0, no product repair

The794 schema quartet passes4/0/0 after a single test compile/clone repair.
The floor trio retains old exact6/start1:1 expectations: Preview reads actual
Copy5 and the first Fence directly selects6; Takeover with older Preview first
sends2 then the same operation retries6 from5; Move receives an exact correlated
Release, Copy is independently fenced5 while Release is held, its exact final1:1
is acknowledged, then Fence2→6 is observed. Actual target Renew6 must report the
whole usable original-duration lease. Old Piece remains stale5 then6 at cutoff1:1;
Copy's empty committed schema1 snapshot remains exact. No fixture floor edits.

The initial floor run failed with localhost AddrNotAvailable before oracle
stages. A serial retry passed3/0/0, but ordered cases then repeated that same
initial clock connection failure even serially. Aggregate host TCP inventory
was not evidence of exhaustion, so no exhaustion claim is made. A temporary
bounded diagnostic in the existing fixture compared default and no_proxy
clients against the same listener: both failed in the sandbox; the identical
command/source succeeded with explicit host loopback permission. The probe
was removed exactly. This establishes the usable execution boundary; the
specific OS cause remains unproven. No production HTTP/default-proxy change
was made. Test-owned loopback access is granted through actual tool permission.

With original fixture restored, the complete three-target runtime command,
transfer-group3-native-first-complete, passes9/0/0 exit0, default test scheduling:
floor3, ordered2, schema4. The ordered cases retain all numeric1/2 and literal
before-left/right, late-left/right and new-left/right values, alongside current
valid UUID keys: complete two-row read while exact Fence2 is held then PreviewStale;
Fence2 first, whole late Piece StaleEpoch2/cutoff1:1 with empty snapshot, then
both new-holder rows through2:1 read on the target socket and original socket.
This is separate from already accepted row11's overlap values and cases.

No pre-fix product defect is observed; the old never-enrolled SchemaTooNew(None)
label retires only under794. All other old expectations remain. Six planned
ordinary Core wrong builds will challenge the nine cases before restored
checks, leaf/declaration/window return. Full workspace/Worker/OS/DO/Router and
whole rows5/9/10/12 credit remain absent. Runtime/pin/protocol/control/schema
and all protected apparatus/suffix bytes remain unchanged atfffae2d1.

## Group3 candidate: nine cases and six effective wrong builds

Current restored runtime proof is9/0/0 exit0 in
transfer-group3-native-restored-retry-final: schema4, floor3, ordered2, default
scheduling. No pre-fix product defect is observed. First complete nine-case
execution was also green on unchanged runtime;794 alone retires the old
SchemaTooNew(None) label for a never-enrolled installation. That producer is
retained as its own exact NotFound/no-grant/unchanged-Copy case. A separate real
known enrollment without Hello/report reaches the actual Fence with supportNone
and preserves typed SchemaTooNew(None), Copy and the original Registry grant.
Known0 and the correlated holder SchemaTooNew response/replay are distinct cases.

Six ordinary Core wrong builds compile and fail for their intended reasons,
with exact source restoration:
- absent enrollment relabelled EpochUnprovable:0/1/0 exit101 at exact NotFound;
- actual Fence permits unknown/zero schema:2/2/0 exit101, both support cases
  observe the forbidden real epoch2 grant rather than their exact refusal;
- correlated holder SchemaTooNew relabelled MigrationPending:0/1/0 exit101;
- first outgoing Fence after Preview5 selects2:0/1/0 exit101 at the actual held
  request, after the real Preview5 observation;
- both actual retry Fences select2 instead of6:1/2/0 exit101, Takeover and Move
  fail after observing their first Fence2 against actual Copy5; direct Preview
  selection remains a passing control;
- actual snapshot truncates two-row results to one:0/2/0 exit101, both before/
  after literal-row assertions fail. This is partial-ordered-rows-rerun, the
  effective post-port-pressure run, not the earlier setup failure.

Clippy found unchecked row[0] indexing in the new ordered helper. It was removed
by zipping the same literal two UUID keys with the same literal two rows, with
no lint waiver or payload change. The first five controls do not consume this
separate ordered integration file; their exact target/shared-helper and all
product source hashes remain current. The audit records only this precise
unconsumed test-file delta, with both hashes and patch. The effective sixth
control and final nine-case run use the corrected ordered helper. Strict
Core/Conformance/Tokio all-target conformance Clippy -D warnings, fmt/Ruff,
whitespace and freshness pass with exit0.

The earlier permission-alone diagnosis is explicitly superseded. Lead799
measured another project's workerd vitest e2e holding about32.9k loopback
connections and exhausting the shared ephemeral ports. B's own later preflight
measured32,905 lines, held the runtime work, then observed5,912 and377 before
retrying; final gate began at701. Earlier sandbox and host executions that
failed with AddrNotAvailable49 before setup/scenario markers receive no runtime
or sensitivity credit. Default/no_proxy both failed in the same process; a later
host-permission success did not establish causation. No network policy or
production process changed. If pressure returns, use799's netstat count; above
about15k wait rather than repeatedly building/running. One notification failed
with Router MCP transport error during pressure; bounded later message history
contained no retained push for it, and it was not replayed. Current declaration
carries the resolved facts. All relevant raw failures remain retained.

The old transfer dispatcher's held-overlap branch remains separately accepted741.
Its owner_fence_ordering.rs and normal creation helper are byte-identical to
937e78dd; runtime/manifest inputs still match the accepted row11 final receipt.
That earlier3/0/0 overlap proof is explicitly reused, not added to these nine
new cases or called a fresh overlap run. Only the new ordered cases retain the
old distinct before-/late-/new-left/right rows. Physical OS restart, Worker/DO,
real Router clients, hosted/staging and whole rows5/9/10/12 stay unverified here.

This leaf adds five permanent test/support files and updates the three B docs.
No product runtime, public protocol, schema, manifest, lock, pin, guard, new
conformance control or private-library SQL changes. Existing public CopyFence
arranges out-of-band5; all claimed actions are real owner HTTP and admitted
session traffic. Refusal history uses read_only stopped Registry connections
and public epoch_holder, with literal original grant and no grants2–6. No Host
stand-in supplies a result. Verifier apparatus and entire observations suffix
remain byte-exact atfffae2d1. Prior Worker/native production/image observations
are explicitly reused at unchanged inputs, matching four bundle/tool hashes;
no fresh Worker/full-suite/workspace run is claimed.

Remaining769 DAG envelope and public missing-Copy/no-grant groups, full row9
revocation mapping and rows10/12 still remain. The private-floor-loss mechanism
stays library-owned. Final A539/555/670/OC-8, RouterG3/OB8, all-touched-package
CI-feature737, D mixed16 HTTP import, independent review/readiness remain separate.
No full slot was taken for this native leaf.

Evidence: tmp/pr3-closure-645/transfer-group3-{proof-map,runtime-summary,
final-freshness,precommit,declaration,network-boundary,ordered-lint-fix}.json;
first complete and effective restored-retry-final logs; six effective wrong
source/patch/restore receipts and all excluded setup failures. Lead decisions792,
794,799 are saved with exact references. The observed final working bytes and
parentfffae2d1 bind to the later immutable target; no postcommit test is invented.
Full unchanged canonical r4 and exact continuation are in the central-transfer-
boundaries handoff. Paired observation/Lead acceptance remain pending.

B returns execution/build/test/generation/Worker window on immutable declaration,
keeps one authorized dependency wake and stops for that actual dependency.
No board/direct Verifier DM, helper, parent design/plan edit, early final pin,
push/install/deploy/tag/default merge or production operation occurred.

## Groups4/5 resume from accepted805 reference

Lead805 (01a15ca9-0598) accepts782a07a0 on Verifier reference49bc1d4e and
explicitly returns B's execution/build/test/generation/Worker window. Pending
wake01a15ca8-4c3a was cancelled before firstFire/no dispatch. Clean HEAD49bc1d4e,
library68394e0, unchanged protected apparatus/observations suffix are the base.
No commission gap: Implementer B owns product/test/proof corrections, direct
Lead messages only; paired Verifier owns apparatus. Group4 is explicit current
DAG envelope mapping, then group5 public missing-Copy/no-grant per769. No full
slot, new private state hook/query, early final pin or production operation.
Before every run, use799's host netstat count and hold above about15k.

Full old call_edge_scenarios.rs and unproven_transfer_rows.rs are read. The old
edge oracle is exact typed CallEdgeRefused{Copy,Registry}. Current public
validate_call_edge preserves that shape; actual ObjectRuntime::on_call decodes
ObjectCallEnvelope first and maps invalid edges to NotSent/Unreachable(None).
Read object_transport.rs:71–99 and public Registry on_call, call_graph.rs,
caller_context.rs and the current Registry handler source checks. Every current
Registry handler independently requires Edge+Owner or Session+Object/source
Connection, so a copied forged Copy+Object envelope can remain NotSent even if
only decode's may_call predicate is bypassed. Do not mislabel that vacuous
runtime result as isolated proof of the outer DAG predicate. No extra seam has
yet been added; the old typed oracle will run first, then the source/proof issue
will be returned to Lead before any new public conformance wrapper is introduced.
Group5 private floor-loss mechanism stays library-owned; public missing-Copy
must use the settled Unavailable outcome, unchanged lease/fence and no grant.

### Decisions806/807: isolated decoder proof and honest missing-Copy producer

Lead806 declines the proposed public wrapper. A cfg(test) module calls the real
ObjectCallEnvelope::decode beside its owner; no new API or production behavior.
The exact typed old Copy→Registry oracle remains separate. Separate public
Registry on_call sends a well-formed Copy/Object envelope and observes exact
NotSent/Unreachable(None), with allowed Session/Object peer-read control.
Initial own runs: typed1/0/0, decoder3/0/0, public1/0/0. No pre-fix defect observed.
Deliberate typed bypass fails0/1/0; deleting only outer may_call fails decoder
2/1/0 while public refusal remains1/0/0, directly demonstrating the independent
handler defense. A deliberate public success for Copy fails0/1/0. Source restored
and all three targets green again1+3+1. A test-only Clippy panic allowance then
matches existing test conventions; final checks still pending.

Lead807 accepts moving only the exact own test Copy file outside the scanned root
after graceful shutdown. Public missing-Copy Takeover must be typed Unavailable,
no target grant, unchanged stopped public lease_status/epoch_holder before/after,
target reconnect empty Welcome and public Copy NotInitialized. Restore only with
all owners stopped; unchanged original fence must be read publicly, then the
same actual Takeover must complete. Record today's Pending/retryable transfer
state. Retire only old same-live-socket Renew/no-active-transfer under769/807;
private Registry floor-loss stays library-owned. Group5 not yet implemented or
run. Lead question01a15cb3-9c5e and decision01a15cb4-114b retained. No stand-in or
private-library SQL, new state hook, public export, full gate or production action.

### Decision808: public-session and underlying-library Copy states

The first group5 draft run reached normal creation and the removed Copy, then
failed before Takeover: it expected session CopyStatus(NotInitialized), whereas
copy_read_calls.rs:70–74 deliberately maps that library state to the public
ReplicationAnswer::CopyUnavailable{reason:Uninitialized}. No Takeover/no-grant/
recovery credit from missing-copy-first (0/1/0 exit101). Lead808 confirms this
mapping and authorizes exact public CopyUnavailable(Uninitialized) plus underlying
public receiver copy_status NotInitialized on the stopped read-only owning store.
Every other807 assertion remains, including actual same-request recovery. No
product repair, wrapper or private query. This is a wrong drafted oracle corrected
by explicit Lead settlement, not a demonstrated pre-fix product defect.

### Groups4/5 candidate: typed DAG, actual decoder, public dispatch and missing Copy

Own final unique native cases6/0/0 (typed1, decoder3, public on_call1,
missing-Copy1), all exit0. Six precise wrong builds caught with exit101 and
intended assertion failures, then exact source restored. The outer-only may_call
deletion is the decoder2/1/0 red while the public case stays1/0/0; this directly
shows the inner Registry defense rather than miscrediting it as isolated DAG
proof. An indiscriminate decoder refusal fails both allowed controls1/2/0,
then restores3/0/0. Separate typed bypass and public-success bypass both fail0/1/0. The two
Copy controls replace retryable HTTP503 with terminal Refused(Unavailable), and
misreport restored grant3; both fail0/1/0 at their named outcome. Initial and
earlier final controls remain retained; proof-prefixed receipts bind final bytes.

Group5 uses only actual normal Fleet/Register/schema1/Preview, test-owned Copy
file loss after all owners stop, public HTTP/session paths, public library reads
on stopped read-only connections and exact original-file restoration. Absence
returns session CopyUnavailable(Uninitialized) and owner HTTP503 typed Unavailable
for the same transfer, twice. A fresh target reconnect has empty Welcome after
the attempted transfer. Stopped receiver copy_status is NotInitialized; exact
public lease_status and epoch_holder grants are unchanged (original1/no2–6),
and holder_change_answer records Pending. Restored original bytes and public
status retain fence1/holder/schema/prefix before any retry. The existing pinned
fixture clock advances five seconds for the real retry deadline; the same full
Takeover completes Granted2/start1:1. Real Copy fence2/target/prefix and stopped
public Registry lease/history/outcome2/no3–6 establish actual recovery.

The first missing-copy-first0/1/0 failed before Takeover on the drafted wrong
session NotInitialized oracle.808 binds its current CopyUnavailable mapping;
that failure is not product-defect/red-first proof. No pre-fix product defect
observed.769/807 retire only old RegistryEpochUnproven and same-live-socket Renew/
no-active-transfer for this destructive case. Private floor-loss stays library-
owned; no private query, state marker, new public seam/export or product repair.

Strict three-package native Clippy(all targets/conformance), formatter, proof-
script Ruff and whitespace checks pass0; exact receipts/freshness audit retained
under tmp/pr3-closure-645/call-edge-missing-copy-*. Production source is unchanged
before the appended cfg(test) decoder module; manifests/lock/pin unchanged.
Verifier apparatus and complete observations suffix remain byte-exact49bc1d4e.
Rust/SQLx/Python inventory: no new dependency, cast, non-exhaustive switch, raw
library query or protocol duplicate; raw JSON only at the tested serialized
envelope. Test-only panic allowance follows existing assertions convention.
Read-only scopes close each actual connection; runtime owners stop before file
moves/restoration. No stand-in returns any claimed outcome. Own TCP pressure
checks preceded each runtime/gate; all stayed below799's15k hold threshold.

This coherent leaf maps769 groups4/5, not whole-row acceptance. Full row9
revocation unrelated-installation reconnect and rows10/12 remain; final A pin,
RouterG3/OB8, all-touched-package CI-feature737, D mixed16 HTTP import and review/
readiness are separate. No fresh Worker/image/OS restart/DO/hosted/staging proof,
full/workspace slot, early pin, parent plan/design edit, board/direct Verifier DM,
helper, push/install/deploy/tag/default merge or production operation.

Immutable declaration follows with B execution/build/test/generation/Worker
window explicitly returned. One authorized dependency wake and stop await actual
paired observation/Lead acceptance plus explicit window return. No tracked write
after declaration. Continue from the later reference only after those releases.

## Accepted813: row5 closed; resume row9, then12, then10

Lead813 (01a15cd8-1682) accepts immutable3a831eae with Verifier reference
fa0a0746 and explicitly returns B's execution/build/test/generation/Worker window.
Wake01a15cd3-f00b was cancelled before firstFire; no dispatched deliveries.
Clean basefa0a0746, library68394e0. Row5 closes at accepted leaves750,757,761,
768,781,792,805,813; full all-touched-package CI-feature gate737 remains owed.
No independent acceptance is inferred from the reference alone:813 states it.

Next scoped order: row9 full revocation/unrelated-installation reconnect; row12
same-socket read before honest reconnect; row10 complete normal/redeploy bodies
across local server reopen. Begin with the complete old3ca2acea oracle, return
any mismatch to Lead before changing outcomes, use real public owner routes,
wrong builds, coherent leaves/declarations and netstat pressure check before runs.
No commission gap; existing Implementer/pairing/collaboration/trace/Rust/tests/
proof standards remain in force. Verifier apparatus and the complete observations
suffix are protected relative to new basefa0a0746. No full/workspace gate without
Lead slot, private-library SQL/new markers/public seam, early final A pin, parent
plan/design edit, board/direct Verifier DM, helper or production action.

Old local_conformance.rs:143–169 invokes staging-safe shared groups, held renewal
replacement and full run_staging_revocation_row. The latter's full283–316 body
is read from the byte-exact retained old-staging-scenarios.rs: normal enrollment
of temporary and unrelated A, owner Revoke→MachineRevoked, typed revoked live
close, fresh revoked upgrade refused, unrelated A connects. Current accepted
session_credentials revoked_registered_machine case retains the first three
and real Copy preservation, but reads only an already-open unrelated target.
The missing piece is a fresh Access-verified target upgrade/Hello after revoke.
No outcome mismatch found in source; no new run or row9 passing credit yet.

### Row9 revocation reconnect candidate under813

The exact old3ca2acea staging_scenarios.rs:283–316 oracle is retained: owner
revokes a normally enrolled live installation, MachineRevoked is returned,
typed CredentialRejected(Revoked) closes the live session, a fresh revoked
upgrade is refused, and unrelated A still connects. The current registered
fixture already retains those refusal/close and Copy-preservation assertions;
the missing fresh A upgrade/Hello is added after them. Old assertion body is
byte-exact after removing only the added helper call; no old predicate weakened.

Fresh unrelated target uses its original normal enrollment/credential and real
Access-verified HTTP101 upgrade, new SessionId Hello, exact empty Welcome with
that ID, then actual Copy status/schema1/prefix1:1 and empty snapshot. The real
replacement socket becomes the fixture's owned target for shutdown. Existing
test-only target visibility changes only to pub(super); no product API or seam.

Old-oracle-first selected case1/0/0 exit0; no mismatch or pre-fix product defect
observed. Three deliberate owner-local wrong builds each fail0/1/0 exit101:
wrong live Revoked close reason; allowing the revoked fresh upgrade; and making
a real revoked record for751 poison another installation's fresh authentication.
The last reaches all previous revocation assertions before the exact unrelated
upgrade fails401/Revoked. It uses the existing central-owned generated enrollment
read and typed decoder, not private library SQL, a test flag or fake response.
All product bytes restored exactly. The complete affected credential target then
passes3/0/0 exit0 (one extended case, two unchanged cases rerun). No new test-count
inflation or fresh combined execution of earlier accepted shared groups claimed.

Strict three-package Clippy(all targets/conformance), formatter, proof-script
Ruff and whitespace checks pass0. Final source inventories match; production,
schema, manifests, lock, library68394e0, Verifier apparatus and the entire
observations suffix remain unchanged relative to fa0a0746. Rust inventory:
no new dependency, cast, non-exhaustive switch, duplicated protocol or domain
state; typed public frames/decisions preserved, Value only for existing raw HTTP
boundary diagnostics. No stand-in supplies any claimed outcome.

Row9 maps its accepted shared groups through813, accepted replacement761 and
this complete revocation candidate. Whole-row acceptance belongs to Lead after
paired observation. Row5 is closed by813; full CI-feature gate737 remains owed.
Next are row12's same-socket read, then full row10 normal/redeploy bodies. Final
A pin, RouterG3/OB8, D mixed16 HTTP import and review/readiness stay separate.
No fresh real Access/Worker/OS/DO/hosted/staging/full-workspace run, helper,
early pin, parent/apparatus/suffix edit, board/direct Verifier DM, push/install/
deploy/tag/default merge or production operation. Host TCP pressure checked
before every runtime/gate, always below799's15k hold threshold.

On immutable declaration B returns execution/build/test/generation/Worker window,
keeps one authorized wake, and stops for paired observation plus actual Lead
disposition and explicit window return. No tracked writes after declaration.

## Accepted818 and producer821: row12 resume before row10

Lead818 (01a15ced-9148) accepts224cb398 on Verifier referencec4d99232,
closes row9's owed piece and explicitly returns B's window. Previous wake
01a15ce4-5354 finished after firing; no active own wake. Clean basec4d99232,
library68394e0. Full old613-line NormalRedeployState and original row12/row10
local_conformance bodies at3ca2acea are read; retained old normal file is exact.

Row12 includes the complete old prepare/verify path, not only an empty read:
owner import/Claim2 on two projects, multi-change rows and stale Preview,
successful Takeover3/replay, old stale cutoff2:3, other-project writes/unread
answer/replay, B in-flight Piece3:2, then correlated same-socket exact complete
prefix3:1 or3:2 before any new Hello, followed by honest reconnect, replay and
new Piece3:3, complete rows, saved Takeover replay and stale old holder.

The B router has no owner import route;821 explicitly admits existing Native
public ImportTask/Copy seed arrangement before startup as setup only, extended
with literal old records(id INTEGER PRIMARY KEY,label TEXT NOT NULL UNIQUE),
imported-1/2/3 at1:3. Real Claim, Preview/Takeover, Pieces/read/reconnect remain
public session/owner HTTP paths. HTTP import stays D's obligation, no credit.
No private SQL, product hook/API or D edit. Keep old epoch/row literals first;
bring any observed mismatch, including possible stale-preview reservation,
back to Lead before changing outcomes. No new run/candidate credit yet.

No commission gap; standard Implementer/pairing/Rust/testing/trace/verification
rules apply. Protect Verifier apparatus and complete observations suffix at
c4d99232. No full/workspace gate without slot, early final A pin, parent edit,
board/direct Verifier DM, helper, push/install/deploy/tag/default merge or
production action. Check host netstat before runs; hold above about15k.

## Row12 setup refusal and approved key amendment824

The literal old integer-primary-key seed reached the actual Copy SeedComplete
call and was refused SchemaUnsupported{form:CreateTable} (row12-normal-seed-
diagnostic:0/1/0 exit101); no Claim or surviving-socket credit. Pinned library
replicated_key.rs requires explicit TEXT NOT NULL keys. Lead824
(01a15cfe-ecdb) approves records(record_key TEXT NOT NULL PRIMARY KEY,
id INTEGER NOT NULL UNIQUE,label TEXT NOT NULL UNIQUE), fixed UUIDv7 keys1–9,
with every old id/label/full row, imported head1:3 and epoch literal unchanged.
One test-local fixed map serves seed/upsert/delete/expected full rows. No
library/admission/runtime/public interface change. Default imported Claim
regression is separately green at row12-normal-default-seed-regression. Earlier
compile/name-collision setup failures have no product-red or row12 credit.
Next run keeps the original epoch3 oracle; any mismatch returns to Lead.

## Row12 retained epoch oracle: observed4 versus old3

row12-normal-old-oracle824 compiled and ran0/1/0 exit101 at c4d99232.
The real UUID-key seed and both Claims2/start1:3 succeeded; exact stale Preview
refusal, complete rows2:2 and old-holder Piece2:3 passed. The subsequent real
Takeover returned Granted4/start2:3, failing the retained old3/start2:3 literal.
Dependent normal-state epochs are unchanged pending Lead disposition. This
is a measured oracle mismatch, not surviving-socket or row12 completion.
Lead was sent the exact result and candidate literal4 adaptation, preserving
every old row, import head, old epoch2/cutoff2:3 and stale Preview behavior.

## Approved826: retire only normal fixture's numeric3 grant

Lead826 (01a15d03-c9df) binds literal4 for actual Takeover, descendant B
positions, stale-newer and saved replay, matching observed reservation behavior.
Only that numeric3 is retired; import1:3, old2:1–2:4/cutoff2:3 and every row
remain. Added a stopped-server read-only public epoch_holder assertion that
epoch3 has no grant; no private query, schema or library change.

## Row12 first green and original-attachment guard

row12-normal-epoch826:1/0/0 exit0 through the full old normal prepare/verify
sequence with approved824/826 adaptations. Original B socket gives its exact
complete permitted prefix before honest reconnect; in-flight replay, new9,
complete final rows, saved Takeover4 replay, old stale4/cutoff2:3 and stopped
public epoch_holder3=None pass. No pre-fix product defect is observed.

Supplemental existing read-only conformance_schema_report asserts exact original
peer/session/attach1/schema1 both before and after that socket read. No Host/API
or query added. Its first draft's schema10 assumption failed at the actual
original schema1 report; holder_reply_scenarios.rs:97 supplies1. Corrected only
that added guard; row12-normal-attachment-corrected then1/0/0 exit0. This setup
assertion correction is neither a product red nor an old-oracle relaxation.

Row12 controls and quality remain next. Row10 remains separate and unproven:
its old caller also includes transport/renewal/window preludes and requires
physical server shutdown, socket closure, same-file reopen and full state
verification. No row10, Worker/DO/hosted/HTTP-import credit follows from row12.

## Row12 bounded controls and restored real path

Controls follow the green result; they show sensitivity, not a pre-fix defect.
Two actual Core Copy snapshot wrong builds compile and run0/1/0 exit101: remove
one row only at4:2, caught by the original socket's complete-prefix assertion;
remove final9 only at4:3, caught after the same-socket stage by the final exact
rows after honest reconnect. Distinct early-reconnect fixture control sends a
real replacement Hello before reading, fails0/1/0 exit101 at the exact original
attach1/schema1 report guard. The allowed-converse Core variant presents the
complete earlier4:1/six-row prefix instead of4:2/seven-row prefix and passes
1/0/0, so the oracle accepts both contracted prefixes. All variant patches,
source maps, raw logs and exact byte-restoration receipts are retained under
tmp/pr3-closure-645/row12-normal-control-*. Every control reached the required
prior normal stage; none is a fabricated reported error or compile/discovery red.

Restored current real path: row12-normal-native-restored passes2/0/0 exit0
(one new full row12 case and one unchanged archived Claim/default-seed regression).
The old schema/key compatibility refusal and old numeric3 mismatch were sent
to Lead and resolved only by824/826; no product/admission/library fix was made.
Strict quality/final freshness and immutable declaration remain pending.

## Row12 final own proof; paired observation remains

Final row12-normal-native-final:2/0/0 exit0 (new normal_redeploy1 plus unchanged
archived_claim1). Strict Core/Conformance/Tokio all-target conformance Clippy
-D warnings, cargo fmt check, own proof-script Ruff/check-format and whitespace
all exit0. Clippy first caught duplicate_mod in the shared fixed-key helper;
the exact nine-key mapping is now defined once in the existing registry setup
helper. This is a test-only ownership correction; no old oracle changed. The
retained controls' acted-on snapshot and attachment assertions are unchanged;
final real route and strict checks rerun after that helper relocation. All
final source inventories match the current files. Known workers-rs template
parse diagnostics are retained separately from actual successful exits.

No production, public protocol, schema/migration, metadata, manifest/lock,
library68394e0, parent plan/design, Verifier apparatus or observations-suffix
change relative to c4d99232. Rust inventory: no new dependency, cast,
non-exhaustive switch, duplicated domain/wire shape or product seam. Real typed
frames/Pieces/grants and existing raw owner HTTP Value boundary remain.
Native public ImportTask/Copy seed is821's setup only; its existing outer
arranger does not supply any claimed Claim/Takeover/Piece/socket/read outcome.
HTTP import stays D's separate obligation. No stand-in proves this candidate.

Scope is row12 only, awaiting paired observation and actual Lead acceptance.
Row10's full transport/renewal/window and stopped/reopened normal state bodies
remain next. Gate737, final A539/555/670/OC-8 pin, RouterG3/OB8, D mixed16 HTTP
import and independent review/readiness remain separate. No fresh Worker/DO,
real hosted Access, OS restart, staging/full-workspace/CI proof or PR readiness.
No board/direct Verifier DM, helper, early final pin, push/install/deploy/tag,
default merge, production action or cleanup. Host TCP pressure before every
run is below799's15k hold threshold; target/debug4.8GiB, disk181GiB free.

On immutable declaration B returns its execution/build/test/generation/Worker
window and stops with one authorized wake. Require actual Lead disposition
plus explicit window return before next tracked work; a reference commit alone
gives neither. No own compiler/runtime job remains at declaration.

## Accepted830: complete row10 normal/redeploy sequence resumes

Lead830 (01a15d21-2b94) accepts39dda133 on Verifier referencee7b7f7be and
explicitly returns B's window. Cancelled obsolete wake01a15d1b-b2de before
first fire; no dispatched delivery. Basee7b7f7be is clean, library68394e0.
Read the full old3ca2acea staging_transport_rows and normal row/renewal/window
bodies, plus row10 caller157–202. Keep old oracle first and return measured
mismatches with anchors; use public routes and821/824 fixture conventions.

Row10 owes all five preludes on the first server: clean Move, exact forwarding
origin/label/post and reply/unchanged Copy, unread owner confirmation response/
replay/conflict/stale Preview, unread R1/replacement/fresh R2/supersession/stale
renewal, and same-socket frame across a zero idle window. Then complete accepted
normal prepare, physical owning server shutdown, actual old-socket close,
same-file server reopen and full reconnect/replay/new rows/Takeover/stale cutoff
body. Accepted826's numeric4 normal fixture adaptation stays; other preludes'
original epochs remain literal until measured and disposed. No credit yet.

Protect Verifier apparatus and whole observations suffix ate7b7f7be; no parent,
library/pin, A/D or public product seam edits. Public Native ImportTask/Copy
seed is setup only, never D HTTP-import proof. After row10 report readiness
for737 and request its slot; do not run full/workspace gate without release.
No helper, board/direct Verifier DM, push/install/deploy/tag/default merge or
production operation. Host TCP pressure before runs; hold above about15k.

## Row10 N1 old-order mismatch returned to Lead

First draft compile failed only test-module re-export visibility, corrected
without oracle change; no runtime/red credit. row10-normal-old-oracle-compiled
then compiled, ran0/1/0 exit101 at N1 after2.58s. Exact imported Claim2/start1:3,
full initial rows, initial owner Pending and actual correlated holder Release
pass. Reply final1:3, then immediate owner call still Pending, failing old
grant3/start1:3 at old staging_transport_rows.rs:52–76. No other prelude or
reopen credit. Current owner drive_due awaits the transient holder RPC bounded
by2s; waiting for initial HTTP Pending therefore outlives its reply correlation.
The Registry test clock is pinned, so a retry's due time does not advance.

Sent exact output/source anchors and candidate current-protocol ordering to
Lead: concurrent owner call and exact live Release/answer, optionally current
active transfer via normal renewal, same attempt completion; keep every old
grant3/start1:3/replay/stale cutoff/full-row assertion. Retire only old initial
Pending-before-answer order/separate Released ack carrier if Lead agrees. No
product defect or repair is asserted; dependent change awaits disposition.
Independent static checks and unchanged accepted row12 regression remain in
scope. All five prelude bodies/full reopen are drafted, not claimed passing.

## Approved832: live RPC ordering preserves the N1 grant oracle

Lead832 (01a15d2f-62b2) retires only initial HTTP Pending-before-holder-answer
ordering and separate Released ack carrier: use concurrent owner HTTP/exact
Release and answer1:3 while correlation is live, then same-attempt completion.
Keep literal grant3/start1:3, replay, full multi-change stale refusal and every
row. Added the optional normal Renew assertion of exact active transfer/old
epoch2 holder before replying. All owner tasks are bounded, aborted/drained
on failure and settled before cleanup. No product change.

Independent unchanged row12 regression1/0/0 exit0. Draft Clippy found one new
raw-JSON indexed mutation; replaced with fallible get_mut at the existing
fixture boundary, same conflicting reason. Full row10 still awaits this run.

## Row10 first complete Native green after832

row10-normal-concurrent832 compiles and passes1/0/0 exit0 (one selected new
case, row12 filtered). All five original preludes execute and pass with their
full literals. N1 checks live active transfer via normal Renew, exact Release/
grant3/start1:3/replay/stale whole refusal/rows under832. N2 retains exact
origin+label+opaque payload+original-frame reply and unchanged Copy; holder
reply is the old fixture stand-in, not board admission or two physical machines.
Owner-loss retains machine denials, discarded response body, exact grant3
replay, old stale Preview, body conflict, full rows and old cutoff. S5 retains
unread R1/replacement/replayed expiry, fresh R2, superseding3 and stale renewal.
S6 retains zero-window same original socket and exact full imported rows, with
read-only attach1/schema1 guard; no elapsed-time/DO-instance attribution.

Then full accepted normal prepare runs on the same first server. Actual
shutdown drains runtime/store, original B socket closure is observed within
10s using the retained old None/Close/transport-error rule, a distinct new
TokioServer opens the same root, and the complete saved in-flight replay/new9/
full rows/saved Takeover4/stale4-cutoff2:3 body passes. Public stopped
epoch_holder3=None passes. No OS crash, hosted/Worker/DO or HTTP-import proof.
No product defect or repair observed.

The common reconnect marker previously claimed a same-socket read even when
called after reopening. Moved that original case marker to actual row12
verify_surviving, retaining its text and assertions; common reconnect now
prints an honest reconnect-stage marker, and row10 separately marks the new
same-root server before reconnect. Bounded controls/final rerun follow; this
marker correction is test evidence hygiene, not a changed oracle.

## Row10 own proof complete; paired observation and737 remain

Six post-green controls compile/run0/1/0 exit101 and fail at their intended
new boundary. Five actual Core wrong builds alter Move start to1:0, forwarding
origin label, replayed owner grant3 to4, renewal replay flag, or final reopened
Copy rows4:3 respectively. Each reaches its prior stage; the last reaches all
five preludes, full normal prepare, first server stop/actual B close and new
same-root server before its missing9 row fails. Distinct fixture control sends
a real new Hello during S6's second-frame path, then fails original attachment
identity after the zero-window marker. Exact source restoration, patches,
source maps and raw receipts are retained in row10-normal-control-* artifacts.
No fabricated reported error, compile/discovery red or pre-fix product defect
is counted. Controls predate only replacing fixture installation String with
validated ServiceId; UUID value, serialization and expected ActorOrigin stay
identical. Final full real route/quality checks rerun after that typing change.

Final row10-normal-native-final:3/0/0 exit0 (one new full row10, retained row12,
unchanged archived Claim/default-seed regression). Strict three-package
Core/Conformance/Tokio all-target conformance Clippy -D warnings, cargo fmt,
own control-script Ruff/format and whitespace checks all exit0. Full final
source inventories match current inputs. Known workers-rs cache-template
parser diagnostics remain in raw output separately from successful exits.
No production, public protocol, schema/migration, metadata, manifest/lock,
library68394e0, parent plan/design, Verifier apparatus or observations-suffix
change relative to e7b7f7be. No cast, non-exhaustive switch, dependency or product
seam. Test identifiers use existing ServiceId/PeerId/SessionId. Raw owner JSON
fixtures intentionally preserve the independent HTTP wire oracle at existing
post(Value) boundary; native TakeoverPreview and all session frames stay typed.

Stand-in retained from old N2: the normally enrolled holder answers the
forwarding RPC with its literal opaque fixture reply. It tests Core transport
and origin/correlation, not board admission/two physical machines. It gains
independent transport proof now; actual board consumer behavior remains
unverified here and belongs to real Router integration/G3. Native public
ImportTask/seed arrangement821/824 is setup only; D HTTP import stays separate.
No stand-in supplies claimed lease, Piece, snapshot or owning-server reopen.

Row10 is an own supported candidate, not Lead acceptance. Ready to queue737
after paired observation/release. Exact737 requires every PR3-touched package's
full --all-targets --no-fail-fast suite with CI features: Core, Tokio conformance,
Conformance, admin protocol, Cloudflare Wasm/Worker journeys and changed shared
packages. None was silently replaced by these narrow leaf gates. No full gate
starts without Lead's serialized slot. Final A539/555/670/OC-8 pin, RouterG3/OB8,
D mixed16 actual HTTP import, hosted/staging and review/readiness stay separate.
No Worker/DO hibernation or OS crash attribution; this is graceful Native server/
store teardown/reopen with fixture Access/JWKS. No board/direct Verifier DM,
helper, early final pin, private SQL/new marker/product seam, push/install/deploy/
tag/default merge, production action or cleanup. TCP pressure checked before
every run, always below799's15k; target/debug4.9GiB, disk177GiB free.

On immutable declaration return B execution/build/test/generation/Worker window,
keep one authorized wake and stop for paired observation plus actual Lead
disposition/explicit return. No tracked writes after declaration. No own
compiler/runtime job remains at declaration.

## Accepted843 and gate737 preparation freeze

Lead843 (01a15d64-bfda) accepts594f41e8 on Verifier reference45e01d34 and
closes rows5/9/10/12. B's window returns. C holds the machine-wide slot842:
prepare and freeze, send exactly B full gate ready; wait for Lead's grant at
C END. No native build/test, Worker build/runtime or full gate has started.
841's no-more-wake-checks remains; wait for direct Lead slot notification.

Verified current PR2 merge base with feat/board-storage is7492ca2b, with158
B-layer first-parent commits above it at45e01d34. Git diff plus current workspace
TOML maps15 active touched packages: board-central-test-support, board-fleet-sync,
board-sync-protocol, fleet-admission, message-board-storage, collaboration-client,
collaboration-mcp, collaboration-service, collaboration-central-admin-protocol,
collaboration-central-core, collaboration-central-cloudflare, collaboration-
central-tokio, collaboration-central-conformance, agent-collaboration and
codex-router-host. Deleted legacy local/protocol packages and dead boundary
source/test retire by508/B6; no missing target is counted as passing.

Command manifest preserves737's --all-targets --no-fail-fast on all15 using
Nextest's CI profile/resource groups, retries0, with Tokio conformance and Host
keychain-test-support (CI enables it through CLI/keychain-test-support). Include
full touched default/enabled-feature lint, checked-SQL freshness, portable Wasm
checks for pure touched crates and no-client Conformance, Cloudflare all-features
Wasm lint, production worker-release/default-feature exclusion, dev fault guard
and every35 current retained Worker selector with exact marker/process settlement.
Root workspace now owns Cloudflare after508; its manifest/CI commands preserve
the same target boundary despite737's older own-workspace wording.

Preparation executes only reads, version/help queries and cached Python-tool
qualification (Python3.13.11/Pydantic2.14.1 with pinned Ruff/Basedpyright). No
application tests/native compile/Worker build. Rust1.98.1, Nextest0.9.137,
worker-build0.8.7, Miniflare5.20261012.0-beta and existing Node26.11.1 are
qualified; local Node differs from CI's22 and does not claim hosted CI. No
Node22 install or global change. Pinned SQLx CLI is missing and its repository
bootstrap is an explicit first setup stage after slot grant, not run now.
Cargo fetch and tool setup remain locked in the granted stage; no global
Router install, secrets, Codex profile or production process is involved.

Freeze changes only these three B proof documents, with product/apparatus
inputs identical to accepted45e01d34 and the complete observations suffix
protected. Prepared gate manifests/source/tool records live in
tmp/pr3-gate737-843; none is another canonical plan or a result. Full gate
remains not-started until the explicit serialized slot; all final A pin,
RouterG3/OB8, mixedD16/HTTP import, hosted/staging and review limits remain.

## Gate737 red baseline and853–858 repair checkpoint

Frozen053c65b5:all62 commands,54passed/8failed/0omitted, aggregate1. Native
2519passed/76failed/1timeout/54skipped; Worker33/35; Python203/0. Slot returned
at01a15da1-e1e9 and confirmed853. Eight-red triage and source origins are in
tmp/pr3-triage853/triage-result.md; raw evidence stays outside target.

Narrow fixes now pass:first-epoch1/0, claim-settlement4/0, Host cleanup1/0,
offline creation target5/0 after all4 intended offline cases fail without the
real disconnect, bootstrap15 groups/2 reaps, SQLite3-schema comparison and
unchanged Turso comparison,14 Python tooling tests, strict four-package Clippy
and formatting/Python quality. Workers allowlist/new direct versions are applied
under857; integrated sources/bans remains red until A's inherited policy/manifest
changes land. Advisory800, delivery-config's unexplained gate timeout, remaining
native failures and record-size timeout are not declared resolved. A's routed
size/protocol/CLI work and final PR2 checkpoint precede the full737 rerun.
