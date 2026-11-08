# D4 central engine

Observe central's real event-to-storage-to-effect path on the declared Tokio or local workerd target. Inbox custody and Report/status routing are D4's boundary; D5 owns delivery to the agent and its order. This route sends no model input.

**Drafted from approved intent; product behavior not yet evaluated.** The original-byte window reader below is executable apparatus. The S1–S11 product controls, complete local scenario controller and isolated wrong-product artifacts still need their declared implementation. Return each missing piece as a gap; the reader's success supplies no whole-journey credit.

The draft S1/control adapters are `event-observation.ts` and `control-client.ts` in the same driver directory. They consume the Implementer's seam contract v3/v3.1, which binds only when its product slice is declared. The HTTP fixture tests establish apparatus serialization and lossless decoding; they do not run the real central. The control client refuses unqualified passivity, separately refuses the legacy clock wake path, and confines Tokio-only controls to Tokio. Never automatically replay an uncertain control mutation.

## Oracle and target

The commissioning packet supplies the central-execution-model design directory and revision: `verification.html`, especially `#d4-notes`, plus `plan.html#d4`, `program-design.html` and `owed-work.html`. Read those actual files before driving. Take expected outcomes from them, including the settled distinctions below, rather than from current test output.

- J5/J13/J18/J26 exact timing, reconstruction and interleaving are Tokio cases. Workerd uses real automatic alarms for shared semantic cases and an aged-row retirement-boundary case; the latter proves no 24-hour production history.
- Resident cadence permits no second batch in 30 seconds. Each actual reconstruction allows at most one extra batch; effects, window membership and retry rules still hold.
- J14 first runs on local workerd; hosted J14/J15 need a separate staging slot and deployed artifact declaration. J15 keeps its real-hour requirement.
- J17 lost-close means the platform socket inventory no longer contains the socket, without a close callback. A socket still reported open is the separate silent-peer backoff case.
- Holder answers are stored and then resume `drive_one`. Rebuild only after the send event has finished, with no active event or call.

J14 still needs a signal bound to the actual JavaScript Durable Object construction. The contract's Cloudflare `isolate` is minted in Rust `DurableObject::new`, which can run after an SDK Rust-state reset while the JS object survives. The Rust counter alone cannot close J14. Keep that gap open until the Lead supplies a qualified signal or an adopted proof change.

Before a product observation, obtain the Lead's immutable slice declaration: full source, signed artifact and bundle hashes, configuration, conformance features, real client, deadline and allocated private stores/ports. Confirm these immediately before and after the run. Build only after the shared Cargo owner releases its slot, in the normal worktree target. Native Rust runs use `scripts/cargo_debug_signing_runner.sh`; sign directly launched binaries with `--sign-only` before hashing. An unavailable signature is a setup gap; this route grants no Keychain prompt, production access or unsigned fallback.

For local launch primitives and tool pins, use [central runtime](central-runtime.md); for real no-model Router inbox fixtures use [D1 central mail](d1-central-mail.md). The generic two-machine/SSH declaration does not describe D4's local two/three-peer rig. A live adapter is ready only when its actual control contract, readiness identity, passive readback and exact owned shutdown are supplied. No D4 launch command is implied by the historical recipes.

## Original-byte window reader

Run from the repository root:

```sh
node verification/observations/central/d4-central-engine/window-readback.ts --help
node verification/observations/central/d4-central-engine/window-readback.ts "$WINDOW_TRACE"
node verification/observations/central/d4-central-engine/wire-event-readback.ts "$ORIGINAL_WIRE_TRACE" "$EVENT_STDERR" "$CONNECTION_BINDINGS"
```

`WINDOW_TRACE` is a bounded owner-private regular JSONL file containing the complete original byte capture, preserving cross-direction observation order for each physical socket. Its records are:

```json
{"kind":"opened","connection":"physical-socket-label"}
{"kind":"client","connection":"physical-socket-label","bytes":"canonical-base64"}
{"kind":"server","connection":"physical-socket-label","bytes":"canonical-base64"}
{"kind":"owedSend","connection":"physical-socket-label","binding":{"attempt":"UUIDv7 of actual attempt","subject":"UUIDv7 of actual owed row","sendId":"UUIDv7 from actual send","kind":"push"}}
{"kind":"ended","connection":"physical-socket-label"}
```

The JSON illustrates record slots; its placeholder values are not a runnable fixture. Copy each byte chunk without decoding/re-encoding its payload. Retain headers and masked client bytes. Bind every Push and status send from the actual S1 record (`kind` is `push` or `status`). Count stable attempt UUIDs: separate attempts for the same subject occupy separate slots, while retries of one attempt replace its send identity. Item/version alone cannot distinguish owed destinations.

When S1 arrives after its wire send, use `wire-event-readback.ts`. It accepts the original trace without `owedSend` records, original stderr containing `centralEvent` records, and a JSON array of actual `{physical,connection,session}` bindings. All three inputs must be bounded owner-private regular files. The physical label maps to the real platform connection UUID and Hello/Welcome session UUID; never infer it from ordering. The joiner matches send UUID, exact original payload bytes, batch group/index/count, subject and publication version, then inserts metadata without changing the order or bytes of any original wire record. It reports S1 sends absent from the wire as diagnostics; they receive no wire credit. The command measures the resulting window and emits `runtimeQualified: false`. It supplies no source/capture provenance or complete event-sequence qualification.

The reader validates UUIDv7 values, both byte streams, actual Hello/Welcome, complete fragments, batch item identities and the peak of distinct pending attempts per connection. A permitted same-session Hello/Welcome replay retains the window; a changed session on that physical connection fails. Retryable refusals keep their slot; stale/cross-connection answers do not release another attempt. Notices, forwarded posts and long-operation release/withdraw requests do not count as owed socket requests. Bytes after a recorded physical end fail rather than disappearing from the measurement.

`SocketWindowObservation` owns attempt/answer accounting, `WindowByteAudit` composes the existing SessionAudit decoder, and `window-readback.ts` reads retained bytes. A caller must separately compare the measurement to a consistent S8 durable snapshot at causal barriers. The CLI labels its result `runtimeQualified: false`: replay alone binds neither source/build nor capture provenance, automatic wakes, a real hour, receiver custody or a wrong-product seed. A trace with no owed requests, invalid/incomplete bytes, absent attempt binding or peak above 32 fails.

The common SessionAudit defaults to single-frame grammar for released-peer compatibility. D4 opts into batch grammar explicitly. Ending an incomplete message stays invalid evidence; a disconnected observer cannot silently turn a partial frame into a pass.

## Local journey coverage to earn

All rows below remain unobserved until their real scenario is driven on a declared product target. The named seam is the prerequisite, not a substitute observation. Use the actual product controls as they land; do not fabricate a successful answer, alarm or effect in the driver.

| Journey | Deciding real observation and required controls |
| --- | --- |
| J1 | Every R1 socket/admin/object request answers with Alarm delivery off; exhaustive kind list, S1/S2. |
| J1b | Each outbound call in an actual Alarm event belongs to a due unit, S1 event/unit/send cause. |
| J1c | Hold actual Project Published answer; other same-socket requests answer before release, S6/S9. |
| J2 | Cleared through 98, notices 99–120 with 99–110 expired and 111–120 live; one frame repairs stale alarm and debt finishes, S2/S5/S8. |
| J2b | Separate next_due, Frame exact-alarm, Alarm exact-alarm and failure-write faults preserve healthy wake and retry; real platform redelivery, S2/S5. |
| J2c | Accepted mail retries after one failed end-of-event exact alarm write with no later input, S2/S3/S5; transactional lower must supply its wake. |
| J3 | Corrupt A notice data; A↔B and B↔C custody and B Hello continue, public Health names A error, S5/S8/S9. |
| J3b | Long-operation step/effect lands while all notice cleanup fails; Health records cleanup, S5/S10. |
| J4 | Work drained: one injected Tokio hour or real workerd hour, zero Alarm entries and no pending alarm, passive S1/S2/S3. |
| J5 | Tokio: 100→one batch, 1,200→three 500-cap batches, singleton by T+30s, duplicate alarm and separate resident/reconstruction controls, S2/S4/S7. |
| J5b | Alarm delivery off; due work completes on prescribed one-second Frames within a cadence interval, S1/S2. |
| J6 | B offline then Welcome; ten real Router inbox receipts once and sender Report/status routing, S6/S8/S9. |
| J6b | 1,001 inbox items; per-connection peak ≤32, central Push order per sender and C↔A progress during B drain; shrinking Health backlog, S1/S8/S9. Agent order stays D5. |
| J7 | Six owner cuts after receiver commit/before saved answer, exact process death/reopen, one effect and finished debt; reconcile nonrepeatable local delivery, S6/S8. |
| J7b status | Drop final status, keep socket, disable fixture polls; quiet retry converges, ack deletes debt and stops retries, S1/S3/S6. Notice half stays D5. |
| J7c | First physical close fails; quiet revoked peer later closes and old frames refuse, S5/S8/S9. |
| J8 | Three actual failed attempts on connected machine produce public stuck count/age/error, unchanged after reconstruction, S4/S5/S8. |
| J9 | Central registers only its five U6 kinds; actual Register/move finish and operation due time feeds alarm, S10. |
| J10 | Shared case receipts agree on both shells with no absent/skipped case; named platform retry differences remain allowed. |
| J13 | Applicable Tokio set runs with actual reconstruction at every quiescent point; nonzero construction evidence, S4. |
| J14 | Tokio event-task/timer probe; local workerd natural idle then next frame constructs again on original sockets/attachments, S4/S11. Hosted slot separate. |
| J15 | Declared staging only: actual Routers, application polls off, one real hour of protocol Ping/Pong, links alive and zero complete handler entries, S11. |
| J17 | Silent-open: peak32 and 30s/1m/2m…10m retry slots; lost callback: absent inventory with no B-row retry alarms for offline hour, S1/S2/S8/S9. |
| J18 | Tokio causal barriers force earlier-due commit versus recompute, step lower versus recompute and overlapping claim; no stale alarm or duplicate batch, S7. |
| J19 | Quiet corrupt expired notice follows attempt backoff, keeps first-failure/error after rebuild, healthy peers unaffected, S4/S5/S8. |
| J20 | Summary call, physical close and settlement each retry from stored deadline without sockets/application input, S3/S5/S6/S8. |
| J21 | Silent-holder/Copy-ack timeouts retry while other frames answer, no event timer/waiter after completion, S4/S10. Active-wait subcase only if adopted path remains. |
| J23 | Two out-of-order publications across quiescent rebuild, stale connection/version replies, forwarded reply and stored holder reply resume; only correct row closes, S1/S4/S8/S10. |
| J25 | Register and Claim answer with current wire results while another silent-holder move waits, S9/S10. |
| J26 | Tokio exact24h failure retirement; workerd aged-boundary variant; given_up has no alarm/window slot, payload/pending status preserved, public admin retry completes, S5/S8. |
| J27 | Failed prune cannot block board/workspace writes or leak expired feed rows; at/before/after exclusive floor plus none/all expired, later contiguous atomic deletion, S5/S8. |
| J22 window | Genuine pre-D3 0.1.134 pair with 100 notices/50 mail, real single Pushes and ≤32 distinct owed items across reconstruction, S1/S8/S9. |
| J22 hour | Same pair for one real hour, no unsolicited notice resend; distinguish poll-read responses and Welcome catch-up, S1/S3. |
| J22 causes | Actual Alarm source/subject evidence shows no released-peer notice-delivery wakes; legitimate polls/retention/other causes remain separate, S1/S2/S8. |

For J22 use the exact old-source/artifact/readiness and no-model qualifications in [D3 wire compatibility](d3-wire-compatibility.md). Its historical local custody run does not cover these D4 window/hour/cause obligations.

## Control qualification and wrong builds

Before a quiet case, prove arm/read/clear bypass `on_event` and cannot repair/recompute, manufacture an alarm or release a batch. Legacy `SetTime` explicitly wakes; the D4 replacement may notify the timer to re-evaluate a stored deadline, but must not create a new deadline. No target polling occurs during the quiet observation. Collect cause/constructor records externally; an inspection that wakes the object cannot prove it hibernated.

S1 sequences start at zero for each `(object, construction)` and increase in completion/emission order; the ring cursor is a separate monotonic emission position across rebuilds. Overlapping events may enter in a different order: retain `enteredAt` and `eventId` and do not use sequence as entry order. This emission binding was supplied by the Implementer for the upcoming S1 slice and still needs source/runtime qualification there. The event reader rejects missing constructions, duplicate event IDs, sequence gaps and truncated ring pages. Each Alarm object call must identify its originating due unit by kind and subject. Preserve those causal bindings; being logged during an Alarm alone is insufficient. The `beforeClaim` barrier sits outside the atomic claim and never inserts an await between its check and set.

Each journey needs its own named wrong-product artifact in an isolated source copy: record original source, exact owner-local change and restored source, build/signing/tool receipts and the same case selection. The seed must reach and fail the journey's semantic assertion while its fixture/control reaches the requested fault. Import/setup/zero-match/time-limit failures do not establish sensitivity. Seeds stay outside the shared Implementer tree. Concrete owner-local seed patches await those product modules; no command here edits them or claims they ran. The byte-reader's encoded 33-item/incomplete-stream controls establish apparatus sensitivity only.

`verification/observations/central/d4-central-engine/seed-qualification.ts` checks the normalized positive, seeded and restored-positive receipts against the predeclared journey/scenario/shell/assertion/readiness. It rejects mismatched base/oracle/configuration, identical artifacts, shared or nested source roots, missing fault reachability, incomplete cleanup, another failed assertion and a failed restored control. Its `semanticFailureMatched` result still says `runtimeQualified: false`: source-copy/patch custody, actual signatures and original live observation evidence must be established separately. Metadata matching never creates the missing wrong-product run.

## Return and cleanup

Keep the declaration, original raw streams, passive event/alarm/construction sequence, arm/reached/consumed records, consistent owner/attempt snapshots, public Health/Report/inbox readbacks, per-J results, wrong-build semantic failures and restored positive results. Name every unknown or absent control. Cleanup closes only recorded owned peers/central/edges after causal settlement and retains evidence; incomplete cleanup prevents a passing journey.

Return each J as supported, unsupported or unknown at its actual target, the exact failed assertion for each seed, source/artifact equivalence, command/count/exit evidence and retained gaps. Until the actual local controller and product controls exist, report this recipe as a draft and only the byte reader as executable apparatus. Independent skill stage review and the live pass remain separate evidence.

`journey-coverage.ts` supplies the required per-shell record keys to the controller and the shared D4 journey-id schema to the seed reader. It keeps each J2b fault, J7 owner cut, J22 window/hour/cause carry, J27 cursor edge, per-journey Tokio rebuild and seeded/restored control explicit. Missing or unknown keys cannot silently complete coverage; a completed record map still has `runtimeQualified: false` until its linked live evidence and target are checked.
