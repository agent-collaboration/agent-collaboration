# Program Design: central automations

Realizes [specification.md](specification.md). The entities come from there unchanged. Revised 2026-10-31 for the Advisor's review (F1–F4, F7–F11) and U12–U18.

## Structure decision

**The Fleet face owns definitions and the clock** (`fleet_runtime.rs:1`, one per organization). **The Registry stays the only mail admitter:** central-produced mail enters custody through a new Fleet→Registry call, so no second admission path exists. **The target Router owns execution,** reusing today's owed-work stores and drivers. Only due-time admission moves to central.

```text
Router (any) ──AutomationCall (Connection→Fleet)──► Fleet: catalog + clock task
                                                     │ alarm (shared AlarmOwner) → admit due → freeze Batch
                                                     ▼ durable step
                                   Fleet→Registry: AdmitCentralBatch
                                                     ▼ (Registry authority, as today)
                                   Mailbox(ServiceId of target machine) ─► Connection ─► machine pulls
target Router: inbox → batch injection (one local txn per entry: receipt + today's owed work)
             → today's wake delivery sender / scheduled run driver
             → FiringReport (seq) ──Connection→Fleet──► Firing outcome
Mailbox status for central items ──Mailbox→Fleet: CentralBatchStatus──► Fleet
```

## Entity binding

| E | Owner and home | Shape (validated by Rust types; no CHECK constraints) |
| --- | --- | --- |
| E8 Fleet | the `ObjectAddress::Fleet` Durable Object (Cloudflare) or native object (Tokio), one per organization | Fleet store tables below |
| E9 Machine | identified by `ServiceId` (`ObjectAddress::Connection(ServiceId)`, `Mailbox(ServiceId)`), resolved by the Registry to its installation peer | the Registry's peer directory (existing) plus a capability flag `automationBatchV1` |
| E1 to E3 Automation, Schedule, Wake | Fleet store | `automations` (`automation_id` UUIDv7 primary key, `kind`, `revision`, `definition_json`, `state`, `creator_json`, `slot_session_key` with a unique index for active and paused Wakes, created and updated timestamps). `definition_json` is today's `ScheduleDefinition` or wake record, with its timing normalized |
| E11 Instruction | Fleet store | `instructions` (`instruction_id` primary key, `current_revision`) and `instruction_revisions` (`revision_id` UUIDv7 primary key, `instruction_id`, `revision`, `text`), moved from the local `instruction_documents` and `instruction_revisions` |
| timing | Fleet store | `automation_timing` (`automation_id` primary key, `next_effective_due_ms`, `admitted_through_ms`, `occupying_firing_id`, `waiting_firing_id`) |
| E4 Firing, E7 outcome | Fleet store | `automation_firings` (`firing_id` UUIDv7 primary key, `automation_id`, `effective_due_ms`, unique (`automation_id`, `effective_due_ms`), `revision`, `instruction_revision_id`, `state`, `batch_id`, `report_seq`, `outcome_json`); finished rows are pruned after 2 months |
| E5 Batch | Fleet store, then the existing mail custody | `automation_batches` (`batch_id` UUIDv7 primary key, `service_id`, `admitted_for_ms`, `frozen_bytes`, `digest`, `send_state`); the mail item ID is `batch_id` |
| operation receipts | Fleet store | `automation_operations` (`operation_id` primary key, `request_digest`, `answer_json`) |
| handover receipts | Fleet store | `automation_imports` (`import_id` primary key, unique (`service_id`, `automation_id`), `payload_digest`) |
| E6 Run, local receipts | the target Router's `automation.sqlite` | today's `workflow_runs` and `mailbox_deliveries`, plus `firing_receipts` (`firing_id` primary key, `batch_id`, `entry_index`, `local_work_ref`, `state`: admitted or done) |
| handover state | the local `automation.sqlite` | `automation_handover` (`automation_id` primary key, `state`, `admitted_through_ms`, `frozen_payload`, `digest`) |
| E12 notification window | the target Router, in the reader delivery owner | `notification_windows` (`recipient_key` primary key, `window_opened_at_ms`, `flush_due_at_ms`) in `interaction.sqlite` |
| E13 Limit | `collaboration-protocol::limits`, one module | constants plus JSON Schema annotations (`minimum`, `maximum`, `multipleOf`, description), used by every constructor |

## Components

| Component | Where | Responsibility (and what it reuses) |
| --- | --- | --- |
| **Limits** | `collaboration-protocol/src/limits.rs` (new) | One inventory: `GRID_STEP = 5 min`, `WAKE_MIN_PERIOD = 10 min`, `WAKE_ONE_SHOT_MAX_AHEAD = 24 h`, `WAKE_REPEAT_MAX_LIFETIME = 7 d`, `SCHEDULE_MIN_INTERVAL = 5 min`, `NOTIFICATION_INTERVAL = 10 min`, `NOTIFICATION_LISTING_MAX_ENTRIES`, `BATCH_MAX_BYTES` (within the 1 MiB unit, `unit_budget.rs:7`), `BATCH_MAX_ENTRIES = 100` (`mail_delivery_session.rs:25–26`), and field sizes. The schemars schema reads the same constants, and validated constructors return `LimitExceeded { limit, value, allowed }` |
| **Automation catalog** | `collaboration-central-core`, the Fleet face | Definitions, Instructions, revisions, the slot and atomic replacement (today's MS-10 rules), and operation-ID replay. Calls arrive Connection→Fleet: `CoreHost::classify` gains the `AutomationCall` kinds for a Session caller, and the installation is bound by the machine session |
| **Automation clock task** | `collaboration-central-core`, registered in the Fleet shared task registry beside `FleetNoticeTask` (`shared_task_registry.rs:62–71`) | Create, change and import upsert `automation_timing` and call `wake_at(next_effective_due)` **in the same transaction** through the shared `AlarmOwner`, so there's never a second alarm owner. On alarm (`drive_due`, at least once), admit every due Firing keyed by (automation, effective due), so a repeat admits nothing. It applies today's fold rules, groups Firings by `ServiceId`, freezes Batches within the size limits, records the send obligation as a durable step, advances timing, and re-arms |
| **Central batch sender** | a durable step in the Fleet; a new **Registry** call `AdmitCentralBatch` | The Fleet asks the Registry to admit the frozen Batch for the `ServiceId`. The Registry checks the peer and its capability and submits it into the Mailbox under Registry authority (`mailbox_submit_runtime.rs:43–70`, unchanged). A lost answer is retried with the same `batch_id` and bytes, and admission is idempotent per item ID |
| **Central status route** | a new Mailbox→Fleet call `CentralBatchStatus`; `StatusRoute` gains a `Fleet` variant beside `ServiceId` | Carries custody status (`sent` at ledger custody, then `received`) to the Fleet for central-sender items. Machine sender checks are unchanged |
| **Mail contract** | `board-sync-protocol` and `collaboration-mail-protocol` | `BoardDeliveryBody::AutomationBatch(AutomationBatchBody)`, addressed to a `ServiceId`. Each entry: `firing_id`, `automation_id`, `revision`, `target` (`SessionRef` or `EndpointRef`), `expires_at` (Wakes only), and the payload: a Wake message, or a ScheduleRun (instruction text and revision, model, effort, timeout, destination). An old receiver can't decode it, so the Registry refuses an incapable peer at admission (`targetNeedsUpgrade`) instead of adding a compatibility shim |
| **Batch injection** | `collaboration-runtime` (new), called by the mail runtime's injection owner | For each entry, in **one `automation.sqlite` transaction**, insert `firing_receipts(admitted)` together with today's owed work: a `mailbox_deliveries` row and wake draft (the `wakeup_evaluation.rs:24–35` shape), or a waiting `workflow_runs` row with frozen inputs (the `run_admission.rs` shape). A repeat finds the receipt and lets the existing owed work continue, so it doesn't stall. An expired Wake entry is recorded as expired and never delivered |
| **Execution drivers** | today's `wakeup_delivery_sender` and today's run-step driving in `schedule_timing_worker` and `scheduled_run_worker` | Unchanged. They drive owed work through delivery, preparation, execution, stop and summary, with today's recovery. **Only due-time admission is removed** from the local timing workers; run driving stays |
| **Firing reporter** | `collaboration-runtime` | When owed work changes state, it sends `FiringReport { firing_id, seq, state, summary? }` Connection→Fleet through the durable outbox. The receipt moves to `done` at a terminal state |
| **Automation operations** | `collaboration-service` `wake_operations` and `schedule_operations`, plus instruction operations | CLI and MCP shapes are kept, plus operation ID, `outcomeUnknown` and `alreadySent`. The calls go to the Fleet; the `remote.rs` request path and the "another service" refusal are retired |
| **Handover** | `collaboration-runtime`, a startup step after enrollment | See the flows below |
| **Notification limiter** | the Router's reader delivery owner (`reader_delivery_owner.rs`), the last stage before a push to a session | For E12 kinds only, it applies the per-recipient window: the first item goes immediately; later ones are held as references (custody unchanged) until `flush_due_at`, then delivered as one listing push up to the listing limit, with a pointer for the rest. Today's per-root quiet and cap windows (`subscription_facts.rs:11–45`) still run upstream, and the replication gate is unchanged |

## Flows

1. **Create or change.**
   1. The Router sends an `AutomationCall` with its operation ID.
   2. The catalog replays a known operation ID: the same digest gets the stored answer, and a different digest gets `operationConflict`.
   3. Otherwise it validates the Limits and the slot, writes the definition, timing, `wake_at` and the operation receipt in one transaction, and answers.
   4. On the Router side: an unreachable central before the send returns `centralUnreachable`, and a lost answer returns `outcomeUnknown` plus the operation ID.
2. **Due (R7f, R6a).**
   1. The alarm fires and `drive_due` runs the clock task.
   2. In one transaction it admits the due Firings (unique on automation and effective due), folds as today, groups them by machine, freezes the Batches (bytes and digest), records the send step, advances timing, and re-arms. That commit is **the cutoff**.
   3. The send step calls `AdmitCentralBatch`. Its answer or the Mailbox status marks the Firings `sent`. A lost answer leaves them `unknown` until a status read reconciles them.
3. **Pause, cancel or change.** These take effect in the catalog. Firings not yet frozen into a Batch are dropped or updated. Firings already frozen are answered as `alreadySent`, and nothing is sent to the Machine.
4. **Receive (R7–R9).** The Machine pulls (existing). Batch injection runs per entry. Today's drivers deliver the wake push or run the schedule; continuity comes from the previous completed run of that schedule on this machine (`run_admission.rs:105–143`). The reporter reports each state.
5. **Handover (R11–R12).**
   1. In one local transaction per Automation: set `pending`, record `admitted_through_ms` (the last locally admitted due time), normalize the timing under U12 and U13, and **freeze and store the payload** (definition, Instruction revisions, `admitted_through_ms`, adjustments).
   2. Locally admitted work keeps running under today's drivers.
   3. Send `AutomationImport` with the frozen bytes. The Fleet inserts it; a repeated digest gets `alreadyImported`, and a different digest gets `importConflict`, which stays `pending` and is reported.
   4. The Fleet admits only due times after `admitted_through_ms`.
   5. Mark `moved`.
   6. A crash between steps resumes with the same bytes.
6. **Notification window (R6n).** An E12 item reaches the reader delivery owner.
   - If no window is open for that recipient, it's pushed now and a window opens until now + 10 min.
   - If one is open, the item is held as a reference.
   - At `flush_due_at`, one listing push goes out and the window closes.
   - Off and Poll policies are applied first, as today.

## State

- **Firing:**
  - pending → sent → (Wake) delivered or expired;
  - pending → sent → (Schedule) run started → run finished or failed;
  - pending → unknown → sent;
  - pending → dropped, by a pause or cancel before the cutoff.
- **Reports:** applied by `seq`; anything at or below the current one is ignored.
- **Local receipt:** admitted → done.
- **Handover:** none → pending → moved.

## Failure and edge behaviour

| Case | Behaviour |
| --- | --- |
| A duplicate alarm | admission is unique on (automation, effective due), so nothing new; the send step resends the same Batch bytes |
| The send's answer is lost | retried with the same `batch_id` and bytes; reconciled through `CentralBatchStatus` |
| A crash after the receipt, before the work | can't happen: the receipt and the owed work commit together. On restart, the existing driver resumes the owed work |
| A crash after native acceptance, before the outcome is stored | today's uncertain and reconcile handling in the drivers; never a blind repeat |
| The Machine is offline | the Batch is held in custody. On return, Wake entries past their end become expired and the rest run |
| An oversized due set | split into several Batches at admission |
| An incapable (old) target | refused at create (`targetNeedsUpgrade`), and the Registry refuses at admission |
| An import conflict | stays `pending`, is reported to the operator, and nothing fires on either side |
| A stored row that breaks a Limit | refused on decode with `limitExceeded`, never coerced |

## Runtime behaviour: clock, delivery, cost

- **The clock is a callback, not a loop.** Each object has one platform alarm, held by `AlarmOwner`, which only moves earlier (`alarm_owner.rs:34–51`). Cloudflare invokes `DurableObject::alarm()` (`object_shell.rs:448`), which calls `drive_due` (`operation_engine.rs:210–238`, concurrency 8). Alarms are at least once, so every admission is idempotent. On Tokio, `wake_at` is a timer.
- **The grid concentrates wake-ups:** under U12, a Fleet wakes at most once per 5-minute grid point that has work.
- **Delivery to machines** uses the existing hibernatable socket. Outgoing messages aren't billed, and incoming ones are billed at 20:1. Mailbox and Connection steps run on their own alarms (`mailbox_step.rs:112–119`, `machine_notice_delivery.rs:146–190`), so the cost of one Batch is several object events, not one.
- **Cost:** the planning model is on `firing-options.html`. The real per-batch event, row-write and duration counts are measured on the first A slice (VD).

## Trace table

| Capability | R | Component | Seam |
| --- | --- | --- | --- |
| definitions | R1 | catalog + operations | List/Show from a second Router |
| definitions | R2 | catalog, clock | a Firing with leases expired |
| definitions | R3 | catalog slot and replacement | refusals and replacement answers |
| definitions | R4 | operations + catalog operation receipts | replay and conflict answers; `outcomeUnknown` |
| definitions | R5 | limits + constructors | schema against acceptance; decode refusal |
| cadence | R6g | limits + clock rounding | `automation_firings.effective_due_ms` |
| cadence | R6n | notification limiter | push count per recipient window; listing content |
| firing | R7f | clock task + batch sender | Batch rows, digests, alarm drive report |
| firing | R6 | mail contract + Registry admission + injection expiry | per-entry outcomes |
| firing | R6a | clock commit (the cutoff) + catalog | the command's answer; Batch membership |
| firing | R7 | batch injection + drivers | receipts; effect counts |
| runs | R8, R9 | drivers | the session's input; run summary |
| runs | R10 | reporter + catalog | Show from a non-target Router |
| migration | R11, R12 | handover | frozen payloads; Fleet rows; effect counts |
| migration | R13 | handover absent on old releases | no Fleet rows |
| migration | R14 | PR 1 retention | local reads |
| design-truth | R15 | the docs reconciliation | manifest, grep, scan |
