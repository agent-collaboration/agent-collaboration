# Program Design: central automations

Realizes [specification.md](specification.md). Revised twice on 2026-10-31: first for the review (F1–F11) and U12–U18, then for its correction check (F1, F2, F4, F6, F10, F11).

## Structure decision

**The Fleet face owns definitions, content and the clock** (one per organization). **Central reaches a machine through the existing Fleet→Delivery machine-notice edge, and the machine pulls through Session→Fleet.** Both edges already exist in the sealed call graph (`call_graph.rs:162–168`: `SessionKind=>FleetKind`, `FleetKind=>DeliveryKind`), so no edge or rank changes. The Registry and the mailboxes are not involved in automations, and session-to-session mail is untouched. **The target Router owns execution**, through today's drivers. Only due-time admission and the waiting slot move to central.

```text
Router (any) ──Session→Fleet: AutomationCall──► Fleet: catalog + content + automation_timing
                                                 │ on_alarm → AutomationClock::evaluate(now): admit effective-due → freeze Batch(es)
                                                 │           + admit due-now notice operations + wake_at(next) via shared AlarmOwner
                                                 ▼ durable notice step (journal, due now)
                              Fleet→Delivery: DeliverMachineNotice(BoardNotice::AutomationBatchReady{batch_id})
                                                 ▼ (journaled per machine, a hint only)
target Router ──Session→Fleet: PullAutomationBatches (on connect, reconnect, hint, CursorExpired)──► unacked batches + bytes
             ──Session→Fleet: FetchContents([content_id, digest])──► content by revision
             ──local admission (one txn)──► today's wake delivery sender / run driver
             ──Session→Fleet: AckAutomationBatches + ReportFirings (one frame, every 500 ms or 100 items)──► custody + outcome
```

## Entity binding

| E | Owner and home | Shape (Rust-validated; no CHECK constraints) |
| --- | --- | --- |
| E8 Fleet | `ObjectAddress::Fleet`, one per organization | the tables below |
| E9 Machine | `ServiceId`, the Delivery face of `ObjectAddress::Connection(ServiceId)`; installation bound by the machine session; capability `automationBatchV1` from its enrollment | — |
| E1–E3 Automation | Fleet store | `automations` (`automation_id` UUIDv7 primary key, `kind`, `revision`, `definition_json`, `state`, `creator_json`, `slot_session_key` unique across active and paused Wakes) |
| E11 Instruction, and wake messages | Fleet store | `content_revisions` (`content_id` UUIDv7 primary key, `owner_id`, `revision`, `digest`, `text`). These are immutable, and a Firing references `content_id` plus `digest` |
| timing | Fleet store | `automation_timing` (`automation_id` primary key, `next_nominal_due_ms`, `next_effective_due_ms`, `admitted_through_nominal_ms`, `occupying_firing_id`, `waiting_firing_id`) |
| E4, E7 Firing and outcome | Fleet store | `automation_firings` (`firing_id` primary key, `automation_id`, `nominal_due_ms` unique per automation, `effective_due_ms`, `revision`, `content_id`, `state`, `batch_id`, `report_seq`, `outcome_json`) |
| E5 Batch | Fleet store (**the custody record**) | `automation_batches` (`batch_id` primary key, `service_id`, `frozen_bytes`, `digest`, `notice_state`, `acked_at_ms` NULL until acknowledged). Unacknowledged Batches never expire; acknowledged ones are pruned after 2 months. Content revisions stay while any unacknowledged Batch or live definition references them |
| clock | the Fleet face's alarm handler plus `automation_timing` | **no journal operation for time**: the deadline lives in `automation_timing.next_effective_due_ms` and the shared `AlarmOwner` |
| operation receipts, imports | Fleet store | `automation_operations` (`operation_id` primary key, digest, answer), `automation_imports` (`import_id` primary key, unique (`service_id`, `automation_id`), digest) |
| local projections | the target Router's `automation.sqlite` | today's `schedule_definitions` and `wakeup_definitions` rows, with a new `authority` column (`local` or `central`; a Rust enum, no CHECK). A `central` row is a read-only projection that keeps today's foreign keys (`automation_v1.sql:51,88`) valid, and the local timing workers never time it |
| local receipts | `automation.sqlite` | `firing_receipts` (`firing_id` primary key, `batch_id`, `local_work_ref`, `state`, `report_seq`) |
| handover | `automation.sqlite` | `automation_handover` (`automation_id` primary key, `state`, `watermark_nominal_ms`, `frozen_payload`, `digest`); the payload includes `external_occupant` (the local run ID still occupying) and `carried_waiting_due_ms` |
| external occupancy | Fleet store | `automation_timing.occupying_external_run` (the local run ID) is cleared by `LocalRunSettled` |
| E12 window | `interaction.sqlite` | `notification_windows` (`recipient_key` primary key, `last_delivered_at_ms`, `flush_due_at_ms`) |
| E13 Limit | `collaboration-protocol/src/limits.rs` | the values in Spec R5 |

## Components

| Component | Where | Responsibility (and what it reuses) |
| --- | --- | --- |
| **Limits** | `collaboration-protocol::limits` | The Spec R5 values as constants. The schemars schemas read them, and the validated constructors refuse a violation as `LimitExceeded`. The existing 1 MiB values point at today's constants instead of copying them |
| **Automation catalog** | the Fleet face (`collaboration-central-core`) | Definitions, content revisions, the slot and atomic replacement, and operation-ID replay. Reached by `AutomationCall` CallTargets on the existing Session→Fleet edge: `CoreHost::classify` gains these kinds for a Session caller, which binds the caller's installation |
| **Automation clock** | the Fleet face's `on_alarm` (`fleet_runtime.rs:54`) plus the shared `AlarmOwner` | **An explicit due-evaluation step, not a journal job.** The pinned journal can't defer a job: `admit` makes it due now (`sql_journal.rs:100–118`), and `Retry{after_ms}` is clamped to 60 seconds (`operation_engine.rs:132–140,294–303`), which would poll.<br>• **When the alarm fires,** `on_alarm` runs `drive_due` as today and also `AutomationClock::evaluate(now)` in **one Fleet transaction**. It admits every Firing whose **effective** due time is ≤ now, after the nominal frontier and unique on (automation, nominal due). It applies the fold and slot rules, including external occupancy; batches and freezes; admits the notice operations, which are due now and correctly immediate; advances `automation_timing`; and calls `wake_at(min next_effective_due)`.<br>• **Create, change and import** call `wake_at(their next effective due)` in their own transaction. `AlarmOwner` keeps only the earliest deadline across all owners, and every owner re-requests its next wake after an alarm fires, which is its documented contract.<br>• **Edge cases:** an empty Fleet requests nothing; alarms arrive at least once, so evaluation is idempotent; notice retries keep their own policy, unchanged |
| **Batch notifier** | a durable step in the Fleet | Calls `DeliverMachineNotice` (Fleet→Delivery, which exists) with `BoardNotice::AutomationBatchReady { batch_id }`. Its answer means "appended to that machine's journal", which moves the Firings to held. A lost answer is retried, and reconciled against the machine's notice position |
| **Batch custody calls (batched envelopes)** | Session→Fleet: `PullAutomationBatches { after?, max_bytes }`, which returns the IDs **and frozen bytes** of every unacknowledged Batch, up to the frame budget, with a continuation token; `FetchContents { [content_id, digest] }`; `AckAutomationBatches { [batch_id, digest] }`; `ReportFirings { [FiringReport] }` | Central checks the caller's installation against each `service_id`. The machine pulls on connect, on reconnect, on any `AutomationBatchReady` hint, and on `CursorExpired`. Acknowledgement only after local admission commits. **Batched per Cloudflare's WebSocket guidance:** each call carries many logical items, so one frame and one Connection→Fleet request replace many. The reporter flushes queued reports at `REPORT_FLUSH_WINDOW` (500 ms) or `REPORT_FLUSH_MAX` (100), whichever comes first, together with any pending acknowledgement. Every item is idempotent, so a lost reply is simply repeated |
| **Notice contract** | `board-sync-protocol` `BoardNotice` | A new variant, `AutomationBatchReady { batch_id }`. An old Router can't decode it, so the catalog refuses a target without `automationBatchV1` (`targetNeedsUpgrade`), and no shim is added |
| **Local admission** | `automation-storage` run and wake admission (`run_admission.rs`, `wakeup_evaluation.rs` shapes) | Per entry, in **one** `automation.sqlite` transaction:<br>• upsert the `central` projection row for the Automation (for the foreign keys);<br>• insert `firing_receipts(admitted)`;<br>• for a Wake: insert today's `mailbox_deliveries` row and wake draft (the message from `FetchContents`);<br>• for a Schedule: insert the run **directly as Preparing**, with captured inputs from the batch (the instruction text from `FetchContents` at the frozen `content_id`, model, effort, timeout, destination) and continuity from the previous completed run of that schedule on this machine. A new `admit_central_run` sits beside `admit_waiting_run` and produces the same row shape, so the central revision is never re-read from local tables. A Wake entry past its `expires_at` is recorded as expired instead |
| **Execution drivers** | today's `wakeup_delivery_sender`, `scheduled_run_worker` and the run-step part of `schedule_timing_worker` | Unchanged: Preparing, executing, stop and summary, with today's uncertain and reconcile handling. **Only local due-time admission is switched off** for `central` rows |
| **Firing reporter** | `collaboration-runtime` | On each state change of a receipt's owed work, in the **same local transaction**, it increments `firing_receipts.report_seq` and queues `FiringReport { firing_id, seq, state, summary? }` durably. A flusher sends queued reports as one `ReportFirings` envelope every 500 ms or 100 reports, whichever comes first. A restart continues from the stored sequence and resends anything unacknowledged |
| **Automation operations** | `collaboration-service` | CLI and MCP shapes are kept, plus operation ID, `outcomeUnknown`, `alreadySent` and `targetNeedsUpgrade`. They call the catalog. The remote-wake request path and the "another service" refusal are retired |
| **Handover** | `collaboration-runtime`, a startup step | See flow 5 |
| **Local run settlement report** | `collaboration-runtime` | When a carried external occupant reaches a terminal state through today's reconciliation, it sends `LocalRunSettled { automation_id, run_id, outcome }` (durable, idempotent), which frees central's slot |
| **Notification limiter** | the Router's reader delivery owner | Rolling per-recipient rate (Spec R6n): it delivers at once if now ≥ `last_delivered_at + 10 min` and nothing is held; otherwise it holds the item as a reference and sets `flush_due_at = last_delivered_at + 10 min`. On flush it sends one listing, ≤ 50 entries plus a pointer to the rest, and sets `last_delivered_at`. The state survives restarts. Off, Poll, the per-root windows and the replication gate stay upstream, as today |

## Flows

1. **Create or change.** Over Session→Fleet: operation-ID replay or conflict; validate the Limits and the slot; write the definition, content and `automation_timing`; call `wake_at(next effective due)` through the shared `AlarmOwner`; write the operation receipt. All in one Fleet transaction. No journal operation is admitted for time.
2. **Due.**
   1. The alarm runs `on_alarm`, which drives the journal and runs `AutomationClock::evaluate(now)`. In one transaction, evaluate admits Firings whose effective due time is ≤ now, folds, batches, freezes, admits the notice operations, advances timing and re-arms. That commit is **the cutoff** (U17).
   2. The notifier appends `AutomationBatchReady` to each machine's journal, and the Firings become held.
   3. The machine pulls its unacknowledged Batches, fetches missing content, admits them locally, and acknowledges them (batched with reports); the Firings become received.
3. **Pause, cancel or change.** Applied in the catalog. A Firing still pending is cancelled; one past the cutoff gets the answer `alreadySent`, and nothing is sent to the machine.
4. **Receive.** Fetch, then content fetch (cached by digest), then local admission per entry, then today's drivers, then reports (delivered, run started, run finished or failed, uncertain, then the reconciled state).
5. **Handover.**
   1. **Freeze locally.** In one local transaction per Automation:
      - stop local admission;
      - record `watermark_nominal_ms`;
      - cancel any not-yet-started waiting Run, and carry its due time;
      - name any Run still preparing, executing or uncertain as `external_occupant`, and any pending wake delivery as a carried pending Firing;
      - normalize the timing;
      - freeze and store the payload.
   2. **Import.** Send `AutomationImport`, which is idempotent; a conflict stays `pending` and is reported.
   3. **Central takes over admission.** The Fleet sets the watermark, the carried waiting slot and the external occupancy, and calls `wake_at` for the next effective due time.
   4. **The local occupant keeps running locally.** It runs under today's drivers on its local row, now a `central` projection. At its terminal state the settlement is queued durably, and `LocalRunSettled` is **sent only after the import is acknowledged** (state `moved`), so central always knows the Automation first. If the occupant settled before the import was frozen, it isn't carried at all. Uncertain is never treated as terminal.
   5. **Mark `moved`.** There's no waiting step, and admission has exactly one owner throughout.
6. **Notifications.** As described under the limiter.

## State

- **Firing:**
  - pending → cancelled;
  - pending → (frozen) → held, or unknown → held;
  - held → received;
  - received → delivered, or expired (Wake);
  - received → run started → run finished, or failed (Schedule);
  - an uncertain state may sit between received and its result.
- **Reports:** applied in `seq` order; anything at or below the current one is ignored; terminal states are final.
- **Receipt:** admitted → done.
- **Handover:** none → pending (frozen, import in flight; any external occupant keeps running) → moved.

## Failure and edge behaviour

| Case | Behaviour |
| --- | --- |
| The alarm handler fails, or the alarm is lost after exhausted retries | per-Automation errors are caught and recorded, and the handler always re-arms; any later central request re-arms for the earliest pending due time; no Firing is lost or doubled |
| A duplicate or retried alarm | `evaluate`'s admission is unique on (automation, nominal due); the same Batch bytes |
| A lost notice answer | retried; reconciled against the machine's notice position; unknown until then |
| The machine is offline beyond the notice window | the notice is gone, but the Batch stays in central's custody. On reconnect, or on `CursorExpired`, the machine lists its unacknowledged Batches, fetches, admits, acknowledges, and expires each Wake entry past its end |
| A lost fetch or acknowledgement reply | repeated; idempotent |
| An uncertain local run at handover | stays occupying; no successor until `LocalRunSettled` |
| A crash during local admission | it's one transaction, so the entry is either fully admitted or not at all; a re-fetch re-admits idempotently by receipt |
| A crash after native acceptance | today's uncertain handling, reported as uncertain, then the reconciled state |
| An instruction edited after admission | the Firing keeps its `content_id`; the new revision applies only to later Firings |
| An empty Fleet at the first create | the create writes `automation_timing` and calls `wake_at`; the alarm then runs `evaluate`. No journal operation is involved |
| An old-release target | `targetNeedsUpgrade` at create |
| A stored row breaks a Limit | refused on decode |

## Runtime behaviour and cost

- **The scheduler model** (Cloudflare's "multiple events with a single alarm" pattern). The stored schedule is `automation_timing` (next effective due time and repeat rule per Automation). Scheduling an event means writing its row, then `wake_at`, which sets the alarm only if none is set or the new time is earlier: `AlarmOwner`, constructor-safe because it checks before it sets. `alarm()` processes everything due, then sets the alarm for the next. On Tokio, the same interface is a timer.
- **Retries and self-healing.** The platform retries a failed `alarm()` with backoff (2 s, up to 6 times), only for the latest `setAlarm`, and then gives up.
  - So `evaluate` catches each Automation's error, records it on that Firing (`failed` with its reason), and always finishes by calling `wake_at(next)`. One bad Automation can't stop the others or the re-arming.
  - **Self-healing:** every catalog call and every machine pull ends with `AlarmOwner::wake_at(min pending due)`, a `getAlarm` read and at most one set. If the alarm was lost after exhausted retries, the next request restores it, and the overdue Firings are sent once (admission is unique on automation and nominal due).
- **The clock is a callback.** `alarm()` calls `on_alarm`, which runs `drive_due` and `AutomationClock::evaluate`, and each re-arms through the shared `AlarmOwner`. Alarms are at least once, and nothing polls.
- **Batched machine-to-central traffic:** pulls, content fetches, acknowledgements and reports are envelopes of many items, following Cloudflare's WebSocket best practice (fewer, larger frames; flushing every 50–100 ms or 50–100 messages). That cuts incoming frames and Connection→Fleet requests in proportion to the batch size.
- **Hops per Batch (planning model):**
  - one Fleet→Delivery notice;
  - one notice drive on the connection;
  - one Session→Fleet fetch, plus one content fetch per new revision (cached);
  - per Firing, 1–3 reports.

  There are no Registry or mailbox hops. The real per-class counts come from the A-slice measurement (VD).

## Trace table

| Capability | R | Component | Seam |
| --- | --- | --- | --- |
| definitions | R1–R4 | catalog + operations | List/Show from a second Router; replay answers |
| definitions | R5 | limits | schema against acceptance; decode refusal |
| cadence | R6g | clock (nominal → effective) | `automation_firings` rows |
| cadence | R6n | notification limiter | push count per recipient; listing |
| firing | R7f | `AutomationClock::evaluate` + notifier | Batch rows; `automation_timing`; alarm callbacks |
| firing | R6 | notice contract + batched custody calls + local admission | per-entry outcomes; capability refusal |
| firing | R6a | clock commit (the cutoff) + catalog | the command answers; Batch membership |
| firing | R7 | local admission + drivers | receipts; effect counts |
| runs | R8, R9 | drivers + `admit_central_run` | session input; frozen revision; summary |
| runs | R10 | reporter + catalog | Show from a non-target Router; `seq` |
| migration | R11, R12 | handover | frozen payload; watermark; effect counts |
| migration | R13 | capability refusal + no import | no Fleet rows |
| migration | R14 | PR 1 retention | local reads |
| design-truth | R15 | the docs reconciliation | manifest, grep, scan |
