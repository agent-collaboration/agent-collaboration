# Verification design: agent-fleet-sync consumer fit (SO51–SO58)

**What this is:** the plan for showing that the consumer-fit obligations hold. The obligations are spec "Consumer fit", SO51–SO58, and their realization is program design §13a. They come from the board adopting the library (board spec 3 G33).

**It's a plan.** Nothing in it has been observed. This document covers only the consumer-fit delta. agent-fleet-sync's original SO1–SO50 were proven before this phase existed (SV1–SV7; the merged stack's Verifier reports).

```text
requirements.md (SU7, SU8, SU10, SU11, SU12, SU18, SU20; owner decision 86) ──► spec SO51–SO58 ──► program design §13a ──► this
```

**Target, for every row:** agent-fleet-sync at the W-C head, on Darwin arm64 and Rust 1.98.1, against real Turso files.
- Native rows use the real `FleetService`, WebSocket, `SessionPump`, `WriterSessionCalls`, `ReplicationSender`, SQLx-Turso and Tokio.
- Workerd rows use the pinned Miniflare and workerd versions with the owned Durable Object driver.
- The existing real-path harness is `tests/consumer` (native) and `tests/workerd`. `tests/normal-migrations-worker` already drives the real receiver `apply_piece` on Durable Object storage.
- **The library has no Durable Object session host.** `FleetService` is native-only, and the workerd fixtures host SQL and apply only, with no session. Session-level workerd cases are observed through the first real Durable Object host, which is central's Cloudflare runtime, at the board's gate G3 (plan r3). A verification-only Durable Object session host would be a second, fake host, so none is built.

**Journeys:** the project has no `verify-agent-fleet-sync` skill. The Verifiers reported that gap during PRs A–E. The journeys below are `needed:`. They're lasting, because a second consumer (AgentStudio) will depend on these behaviors. The plan builds them as scripts under the repository's verification location before the observations that depend on them.

## Owner's view: what each row proves

| Requirement | Proven by | Still unverified |
|---|---|---|
| Decision 86, SU11: engine keys are UUIDv7 | Constructing an engine identity from a non-UUIDv7 fails. A stored non-v7 or NULL engine key is refused when read or written | other platforms |
| SU8, SU20: refusals carry what a consumer shows | A move blocked by a pending migration names it, and a holder's `failed` answer passes through unchanged. A claim refused as already held names the holder it saw, even after the holder changes | hosted runs |
| SU8: holder changes respect the target's schema | Two claimants with different supported schemas get different, correct fence decisions. A move's target is decided by its own support, and a repeat keeps its first decision. A delayed report from an older session never overrides a newer one | hosted runs |
| SU20: Released isn't copy-complete | A move shows Released while it's still waiting on the copy, and grants only after the copy's acknowledgement | — |
| SU18: an identical re-import stays identical after a claim | After a claim, re-importing the same ID and digest answers `alreadyImported`; different content answers `importConflict` | — |
| SU12: an identified frame is always answered | A frame with a readable ID and a bad body gets a correlated `invalidShape`, and the session keeps working | the Cloudflare edge |
| SU12, SU18: one copy's failure stays with that copy | With two databases on one connection, a damaged copy's operation gets a typed answer while the healthy database keeps replicating. A claim or move blocked by the copy resumes once it's repaired | real disk I/O faults (injected at the store boundary instead); the authoritative NotInitialized reseed (W-C9, G32) |

## Coverage rows

### SO51: engine keys are UUIDv7
- **Requirement:** owner decision 86, SU11; SO51.
- **Reference:** the owner's rule. Every engine table keyed by an entity identity keys on canonical UUIDv7 `TEXT NOT NULL`. Bookkeeping keyed by a UUIDv7 plus an ordinal or name keeps its key. Plus the engine-key audit's tables A to C.
- **Target and journey:** no journey for the constructor rule, which is a pure rule. For storage: state inspection of real engine tables after real operations.
- **Passes when:**
  - every entity identity constructor, plus `OperationId` and `PreviewToken`, refuses a v4, nil or non-canonical UUID and accepts a canonical v7. `ConnectionId` and `FrameId` refuse only nil;
  - a test `IdSource` that mints a v4 is refused at operation admission, and no operation row is written;
  - reading an engine row whose stored key isn't canonical v7 text is refused as corrupt. That includes a stored `operation_id` that's v4, uppercase v7 or unhyphenated v7; a preview token; and a `status_owed.target` holding a v4 peer. A frame carrying a braced or URN spelling of a v7 identity is refused, not normalized;
  - the engine schema refuses a NULL key in each audited `TEXT`-keyed table;
  - each audited `INTEGER PRIMARY KEY` (a rowid alias) never holds NULL. A NULL insert is assigned a rowid, so the check is a readback after real engine writes: each stored key equals the value the engine bound;
  - the real `_fleet_sync_sessions` after a real session holds only v7 peer and session IDs;
  - a `SeedBegin` carrying a v4 attempt is refused, and no `_fleet_sync_seed_attempts` row or seed receipt is written;
  - an install record whose `install_id` is stored non-canonical (uppercase, or unhyphenated) is refused on read, and a v7 one round-trips;
  - the receiver alone, driven directly without the runner: a `SnapshotInstall` carrying a v4 install identity is refused before any `Starting` marker or `_fleet_sync_install_pages` row is written; a stored `Starting` marker or page receipt holding non-canonical text is refused, not normalized; a v7 install round-trips;
  - an `ItemId` given as a braced or URN spelling is refused, not normalized.
- **Builds that fail:**
  - a constructor that checks only non-nil (today's `replication_ids.rs:10`);
  - validation added only in the runner's `InstallRecord`, which the receiver-only case defeats;
  - `OperationId(pub Uuid)` decoded by `Uuid::parse_str` alone (today's `sql_rows.rs:49, 301`), and a v7 check applied after parsing, which accepts an uppercase stored key;
  - a `PreviewToken` that checks only non-empty (today's `validated_text!`);
  - a schema with `notnull=0` on `_fleet_sync_leases.database` (today's).
  - an engine write that leaves an integer bookkeeping key to rowid assignment instead of binding it.
- **Reuses:** none, because today's constructors accept v4.
- **Unverified:** a store deployed before the change. None exists (§13a's in-place amendment).

### SO52: refusals carry what the consumer shows
- **Requirement:** SU8, SU20; SO52; board FL-10, FL-16.
- **Reference:**
  - a move refused for migration `v7` pending names `v7` and `pending`;
  - a holder answer of `failed` for `v7` reaches the refusal and its replay as `failed`, never rewritten to `pending`;
  - a claim refused while held by peer P names P, and still names P after a later takeover gives the database to Q.
- **Target and journey:** native. `needed: verify-agent-fleet-sync holder-refusals` drives a real move against a holder with a pending migration, and a real claim on a held database. It then changes the holder and repeats the claim, reading the typed refusals and the stored outcome. Lifetime: lasting.
- **`failed` is proven at the registry.** No writer produces it (PD §13a SO52), so no journey can observe it from a real holder. The registry's product test (`turso_move_refusal_tests.rs`, on the real registry, engine and store) answers the `HolderRelease` call with each state and checks:
  - the first result carries the exact migration and state;
  - after the scripted answer is changed, the replay still returns the first result, so the outcome is frozen;
  - no extra release, fence, lease or epoch effect, as today's test already checks.
  - Today's test maps both states to the same unit `MigrationPending` (lines 20 and 27), so it proves nothing about the detail until C2 changes it. A scripted holder at this seam proves the registry's pass-through only. It stands in for a producer that doesn't exist and is no evidence of one; the missing real `failed` producer stays a named limit.
- **Passes when:** the refusals carry exactly that detail, and the repeat returns the recorded holder P.
- **Builds that fail:** today's unit `MigrationPending` and `AlreadyHeld`, a registry that maps `failed` to `pending`, a replay that re-reads the holder's answer, and a replay that reads the current holder (Q).
- **Reuses:** none.
- **Unverified:** hosted runs.

### SO53: per-target schema support
- **Requirement:** SU8; SO53; board FL-16, WA-7.
- **Reference:** a copy at schema 5. Claimant A supports 5 and is granted; claimant B supports 4 and is refused `schemaTooNew{store: 5, supported: 4}`.
- **Target and journey:** native, through `needed: verify-agent-fleet-sync target-capability`. The setup keeps holder contention out of the schema decision (V-C1):
  0. **The admission window (review F1).** Target C's accepted support is 5. An admin move to C is admitted, and the task is held before its `check` step by delaying the engine's drive. C's newer declaration of 4 is then published, and the task is released. It must freeze 5. The same case runs again with a restart between admission and `check`. A fresh attempt admitted after the publication freezes 4.
  1. B claims a **ready, unheld** copy first and is refused `schemaTooNew`.
  2. B's declared support rises to 5, and B repeats the **same** attempt.
  3. A **fresh** B attempt is made (or A, on an independent unheld database) as the positive control.
  
  Lifetime: lasting.
- **Two more cases on the same journey:**
  - **A move target.** A move to an installation whose last accepted declaration is below the copy's schema is refused `schemaTooNew` naming both, and a move to one at or above it proceeds.
  - **Report ordering, the old report late.** Installation C attaches with support 5, then reattaches with a newer build declaring 4. The report from the first attach is held back and delivered after the second; the fence must still see 4. The registry's stored row is inspected for its attach sequence.
  - **Report ordering, the new report pending (the barrier).** C has an accepted row of 5. C reattaches declaring 4, and that new report is held back:
    - C's new connection gets no `Welcome`, and its requests aren't served;
    - a change admitted in that window is ordered before the new attach and freezes 5. This is the linearization §13a states, and the run records it;
    - once the report is released, the row reads 4 and C is welcomed. A change admitted after that freezes 4;
    - C's own claim can only come after its `Welcome`, so it always freezes 4.
  - **No report.** A target with no accepted report is refused with `supported` absent.
  - **Native step-scope controls (PD §13a SO53, C3).** These run through the real native `SessionHost` and the engine adapter:
    - while the report is held, there's no `Welcome` and requests are blocked, but a close and other input on the host still progress;
    - a finalizing step that rolls back after staging publishes neither the registration nor `Welcome`, and its retry succeeds;
    - a close, a replacing attach or a restart before a delayed finalization leaves no resurrected registration and sends no `Welcome`.
- **Passes when:** B is refused with both versions named, and B's same-attempt repeat replays that recorded refusal. The fresh attempt is granted. The move target and ordering cases decide from the right value.
- **Builds that fail:**
  - one global `target_schema_max`, which gives every target the same decision;
  - a replay that re-reads the projection;
  - a projection where the latest arrival wins, which the held-back old report defeats;
  - a `Welcome` sent at attach commit, before the registry has the declaration, which the held-back new report defeats;
  - a projection read in `check` rather than at admission, which step 0 defeats.
  - a finalizing step that holds the runtime lock while it dispatches `Welcome`, which deadlocks the held-report control. A step that commits its journal separately from the registration edit, or a session object with no runner, fails the rollback and progress controls.
- **Reuses:** none.

### SO54: Released isn't copy-complete
- **Requirement:** SU20; SO54, SO48.
- **Reference:** SO48's sequence. Released is the holder stopped at its final captured position, then the wait for the copy's acknowledgement, then fencing and granting.
- **Target and journey:** no new journey.
- **Passes when:** during a move whose copy hasn't acknowledged, the view shows Released while waiting on the copy, with no grant. The grant appears only after the acknowledgement.
- **Builds that fail:** one that grants at Released.
- **Reuses:** `turso_workflow_tests.rs:152–200`, which asserts Released while waiting on the copy with the real Turso registry. It applies while that test runs against the W-C head.
- **Unverified:** views that consumers build (the board's own proof covers its views).

### SO55: identical re-import after a claim
- **Requirement:** SU18; SO55, SO46; board FL-17.
- **Reference:** import X with digest d (attempt a1), then claim it.
  - A **fresh** attempt a2 importing X with d answers `alreadyImported`.
  - A fresh attempt a3 importing X with d′ answers `importConflict{existing: d, offered: d′}`.
  - Repeating a1 replays its own `Imported`. That's a separate control (V-C2), because receipt replay alone could hide a lost import origin.
  - A retired X answers `retired` first.
- **Target and journey:** native. The existing `native_lifecycle` journey gets the post-claim re-import case. The Advisor's IR-17 probe showed this case fails today.
- **Passes when:** each answer is as listed, and no state changes.
- **Builds that fail:** today's code, which gives `importConflict{existing: None}` after a claim.
- **Reuses:** `native_lifecycle.rs` for the pre-claim control.

### SO56: an identified frame is always answered
- **Requirement:** SU12; SO56; board FL-12.
- **Reference:** a request frame with a valid frame ID and a body missing a required field gets `invalidShape{field}` correlated to that ID. A later valid request on the same connection is answered. A frame with an unreadable envelope closes `malformedFrame`.
- **Target and journey:** native, through `needed: verify-agent-fleet-sync frame-shapes`, which sends crafted frames over a real session. Lifetime: lasting. The workerd session case is observed through central's Cloudflare runtime at the board's G3, because the session rule is runtime-neutral and the real Durable Object session host is central's.
- **Passes when:** each case is as listed, and the connection survives the bad-body case.
- **Builds that fail:** today's, which closes the connection for any decode error (`session_rules.rs:66`).
- **Unverified:** the Cloudflare edge. The workerd session case is observed at the board's G3 instead.

### SO57: one copy's failure stays with that copy
- **Requirement:** SU12, SU18; SO57; board FL-12.
- **Reference:** two databases on one connection (X7's setup).
  - A damaged copy's Piece gets a typed `CopyUnavailable` while the healthy database's Piece applies.
  - After repair, the same Piece applies once.
  - A committed apply whose answer is lost replays `AlreadyApplied`.
  - A claim, move ack wait or fence blocked by the copy stays in progress, and after repair the same attempt continues to a grant.
  - A final refusal still replays.
- **Target and journey:** native, through `needed: verify-agent-fleet-sync copy-containment`, which extends X7's and the Advisor's copy-resume probes into lasting scripts. Lifetime: lasting.
  - **On workerd, in the library:** `tests/normal-migrations-worker`'s real `apply_piece` fixture, extended in C6, is evidence for receiver and driver atomicity on real Durable Object storage and for the reason codec.
    - It counts for the producer's `CopyUnavailable`-versus-`OutcomeUnknown` classification only if the extension calls the same production transaction adapter (`DurableObjectStore`) and the same classifier the real host uses, with commit and no-commit controls.
    - Today the fixture flattens every error to a string (`lib.rs:113, 120`). A mapping copied into the fixture would be fixture-owned policy, not proof.
    - Otherwise, that producer-specific Durable Object observation runs at the board's G3 alongside the session cases.
  - Session-level containment on workerd is observed through central's Cloudflare runtime at the board's G3, against the exact C revision. The controls are the same:
    - SO56's bad body, its correlated `invalidShape`, then a valid frame on the same socket;
    - SO57's two-database containment, plus the uncertainty and replay controls.
  - **PR C's readiness:** C's slices can be implemented, and consumed by the board through C's pinned head, before G3 runs. PR C is ready only once those G3 receipts exist. "C slices implemented" is not "PR C ready".
  - **Staging:** the controls that depend on seams arriving in C3 and C6 (the held report and drive, the failing `WakeScheduler`, the store-boundary wrapper) are scripted against those seams as they land, and run against the C3 or C6 head, not against main.
- **Passes when:**
  - the healthy database keeps acknowledging;
  - the damaged one gets the typed answer, never `AnswerLost`;
  - the sender retains and resumes the identical piece identity, and so do the seed paths;
  - the blocked holder change keeps its attempt, epoch and reservation, and grants after repair.
- **Builds that fail:**
  - today's broken socket and `AnswerLost` (X7);
  - a storage error that still ends the socket loop for every database (`session_socket.rs:125–137`);
  - a producer that answers an error after a committed apply whose wake failed;
  - a free-text or untyped `reason`, or a decoder that treats an undecodable reason as no-effect;
  - a terminal `Refused(Unavailable)` claim (the copy-resume probe);
  - a copy call cached as `CallDone` across a restart (F-PD1).
  - a `Retryable` stored as `NeverSent`, which lets the unsent timeout settle it, or as `MayHaveReached`, which blocks a safe non-deduplicating retry;
  - a `Retryable` answer that clears an earlier `MayHaveReached`.

  **Reach-state controls (PD §13a SO57, `NoEffect`; C6).** These run on the real engine and store, natively and on workerd:
  - a non-deduplicating call answered `Retryable`, then a restart, then a safe retry;
  - `NoEffect`, then `NotSent` beyond the unsent bound: the call is still pending, never `NeverSent`;
  - a crash after `Dispatching` from `NoEffect`: recovery is `Uncertain` for a non-deduplicating target, with no second send;
  - a deduplicating target with a real effect: `AnswerLost`, then `Retryable`, then repair. The uncertainty is preserved, and the call converges to exactly one effect;
  - a real journal reopened with `NoEffect` and its reason, natively and on workerd. Existing `NeverSent`, `Dispatching` and `MayHaveReached` rows decode unchanged, and the `may_have_reached` projection stays consistent;
  - an unknown retry-reason code refused by the lease view's closed codec;
  - the existing unknown-boundary and post-commit wake-fault controls, kept.

  **Certainty and containment control (review F3):** a test-only store wrapper reports `Backend` after A's callback on the shared-store path, standing for a commit or rollback failure. Then:
  - A's answer is `OutcomeUnknown`, never `CopyUnavailable`, and no `Retryable` reaches the engine;
  - B on the same connection keeps acknowledging throughout;
  - A's piece is resent under its original identity and reconciles: `AlreadyApplied` when the wrapper hid a real commit, or one apply when it didn't. Both variants run.
  - A's `OutcomeUnknown{reason: StoreBoundary{detail}}` crosses the real session and decodes at the writer with its detail intact. An answer whose reason carries an unknown `kind` is treated as `AnswerLost` and reconciles the same way, never as no-effect (review F6).

  **The post-commit wake fault (review F4), required:** the test-only failing `WakeScheduler` wrapper on the real service host fails the wake that follows a committed apply. The answer is the committed `Applied{through}`, never an error or refusal. The follow-ups admitted in that apply run once, later, on the alarm owner's next wake or after a restart, and the apply is never repeated.

  **Codec controls:** a callback's `StepRollback` with an unknown, malformed or unrelated reason code is refused as corrupt. Every `CopyUnavailableReason` variant round-trips through the codec with its whole payload, including `Damaged{detail}`.

  The committed-answer-loss control proves only sender replay (`AlreadyApplied`). It stays a separate control from the wake fault above (V-C3).
- **Reuses:** none; X7's probes are scratch.
- **Unverified:**
  - the authoritative NotInitialized path, which stays the W-C9 limit (G32);
  - real commit and rollback IO faults: the certainty control injects the outcome at the store boundary, not a failing disk. On the Durable Object path, certainty rests on the platform's rule that a rejected transaction rolls back (the driver's `transaction_rolls_back_when_the_callback_fails` scenario), which the library doesn't prove itself;
  - hosted runs.

### SO58: a write knows the epoch that admitted it
- **Requirement:** SU7, SU10; SO58; board WA-3 (the board review's F1).
- **Reference:** SO44 (a created database is its creator's at epoch 1) and the writer's admission states.
- **Target and journey:** native. No journey: the board's `board-records` journey observes the stamped epochs end to end. This row's own cases are focused tests through the real `ReplicatedDatabase` and Turso writer.
- **Passes when:**
  - a write admitted while writing at epoch e reports e;
  - a provisional write reports 1;
  - reading `admitted_epoch` while the transaction is open doesn't block, and the transaction commits;
  - with a controlled clock, the lease expires mid-transaction. `admitted_epoch` still reports e, and the commit is refused `LeaseExpired`;
  - a supersession the writer learned before `begin_write` is refused at begin;
  - a takeover the writer hasn't learned of (its session to the registry is held) lets the write commit under the old epoch; when the piece reaches the fenced copy it's refused, and those records form the rejected tail (review F7; SO17, board FL-10).
- **Builds that fail:**
  - an accessor that takes the control lock again, which waits on itself;
  - a consumer reading `writer_state` inside the transaction;
  - an accessor computed from `writer_state(now)` at read time, which changes when the lease expires;
  - a commit that ignores expiry.
- **Reuses:** none.
- **Unverified:** hosted runs.

## Limits the owner should know
None of these rows weakens a required observation. Real disk-full and real commit or rollback I/O faults are injected at the store boundary, not produced by a failing disk. The authoritative NotInitialized reseed stays the W-C9 limit (G32). On the Durable Object path, no-effect certainty rests on the platform's rule that a rejected transaction rolls back.
