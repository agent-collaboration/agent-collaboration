# Program Design: central automations

Realizes [specification.md](specification.md) (R1–R15). The entities come from the Specification and keep their meaning here.

## Structure decision

**Central's Fleet face owns automations.** It already "owns central's nonreplicated application state" (`collaboration-central-core/src/fleet_runtime.rs:1`), and it runs on alarms through durable steps. It runs on both central runtimes through the one core (C2). Firing reuses the mail path, with central as the sender. Each Router keeps execution (the wake push and the run) and its local history, and gives up definitions and timers.

```text
any Router (CLI/MCP) ──calls──► Fleet face: automation catalog + clock
                                   │ alarm: admit due Firing (one txn)
                                   ▼
                          recipient custody (central ledger, U63)
                                   │ machine session
                                   ▼
target Router: inbox (interaction.sqlite) ─► firing injection ─► wake push | scheduled run
                                   │ FiringReport (durable)
                                   └──────────────► Fleet face (outcome)
```

## Entity binding

| E | Home | Shape |
| --- | --- | --- |
| E1 Automation, E2 Schedule, E3 Wake | Fleet face store (central, both runtimes) | `automations`: `automation_id` (UUIDv7 primary key), `kind`, `revision`, `definition_json`, `state`, `creator_json`, `created_at_ms`, `updated_at_ms`. The kind, state and definition are validated by Rust enums, with no CHECK constraints (C5). `definition_json` carries today's `ScheduleDefinition` (instruction, timing, destination, model, effort, timeout) or today's wake record unchanged |
| timing (part of E1) | Fleet face store | `automation_timing`: `automation_id` (primary key and foreign key), `next_due_ms`, `watermark_ms`, `occupying_firing_id`, `waiting_firing_id` |
| E4 Firing, E7 Firing outcome | Fleet face store | `automation_firings`: `firing_id` (UUIDv7 primary key), `automation_id` (foreign key), `revision`, `due_ms`, `state`, `mail_item_id`, `outcome_json`, `updated_at_ms`. Finished Firings are removed after 2 months (the PR 1 retention rule) |
| migration record | Fleet face store | `automation_imports`: `import_id` (UUIDv7 primary key), unique on (`installation_id`, `automation_id`), `definition_digest`, `imported_at_ms` |
| E5 Delivery item | the existing mail custody (central ledger) and the recipient's `interaction.sqlite` | a mail item with `MailSender::Automation { automation_id }` and `MailBody::Firing(FiringBody)` |
| E6 Run | the target Router's `automation.sqlite`, the existing `workflow_runs` | plus `firing_id`, unique |
| E10 Local history, and execution receipts | the target Router's `automation.sqlite` | `firing_receipts`: `firing_id` (primary key), `received_at_ms`, `local_state`; the existing history tables are kept read-only and pruned at 2 months |
| handover state | the Router's `automation.sqlite` | `automation_handover`: `automation_id` (primary key), `state` (`pending` or `moved`, validated in Rust), `updated_at_ms` |

## Components

| Component | Crate | Owns | Change |
| --- | --- | --- | --- |
| **Automation catalog** | `collaboration-central-core` (new module under the Fleet face) | the definitions, revisions, the U66 limits and one-active-Wake checks, list and show | new |
| **Automation clock** | `collaboration-central-core` (the Fleet face alarm, a durable step) | admitting due Firings, today's fold rules, advancing timing, the next alarm | new; reuses the `durable_steps` drive in `fleet_runtime.rs:54` |
| **Firing sender** | `collaboration-central-core` | after commit, placing the mail item in recipient custody (the existing `effect_driver/mailbox_requests` path); a cancel for undelivered items; reading the sender status back into Firing state | new; reuses the mailbox request and sender settlement calls (`mail_sender_calls.rs`) |
| **Mail contract** | `collaboration-mail-protocol` | `MailSender::Automation`, `MailBody::Firing`, `MailKind::Firing`; `MailRecipient::Endpoint` for fresh-each-run, where no session exists yet | additive protocol change |
| **Automation calls** | `collaboration-central-core` call targets, plus `collaboration-runtime/src/fleet_client.rs` | `AutomationCreate`, `Change`, `Pause`, `Resume`, `Cancel` (each with `expected_revision`), `List`, `Show`, `Import`, and `FiringReport` | new; carried on the existing single machine session |
| **Automation operations** | `collaboration-service` `wake_operations` and `schedule_operations` | CLI and MCP keep their commands and output shapes, but read and write through the Fleet calls | rewired; the `remote.rs` request path retires at cutover; the "endpoint belongs to another service" refusal (`schedule_activation.rs:80`) goes |
| **Firing injection** | `collaboration-runtime` (new `firing_injection`), called by the mail runtime's injection owner | the `firing_id` receipt (dedupe, R7); a wake goes to today's `build_wake_push_draft` and delivery; a schedule goes to a `workflow_runs` row and today's scheduled-run execution route and summary worker | new |
| **Firing reporter** | `collaboration-runtime` | a durable `FiringReport` for every run or delivery state change, retried until accepted | new; reuses the fleet-sync durable outbox |
| **Handover** | `collaboration-runtime` (a Host startup step after enrollment) | the one-time move of local definitions | new |
| **Local timers** | `schedule_timing_worker`, `wakeup_timing_worker` | skip `pending` and `moved` automations; removed once the machine has none left | retired |

## Flows

**Create, from any machine (R1, R3, R4).** Router → `AutomationCreate` → the catalog validates (U66, one active Wake per session against the Fleet-wide list), inserts the definition and its timing in one transaction, and answers with the revision. If central is unreachable, the CLI and MCP return `centralUnreachable`, and nothing is queued.

**Fire (R5, R6, R6a).**
1. The alarm runs. In one Fleet transaction, for each enabled Automation with `next_due_ms <= now`:
   - a pending undelivered Firing of a Wake absorbs the due time;
   - a Schedule with a `waiting_firing_id` folds into it;
   - otherwise a new `automation_firings` row is created, the occupying and waiting slots are set, `next_due_ms` is advanced from now (today's rule), and a mail-submit obligation is recorded.
2. After commit, the Firing sender places the item in recipient custody.
   - Its `valid_until` is the Wake's end, or the Schedule's next due time.
   - Then the next alarm is set to the minimum `next_due_ms`.
3. A pause or cancel arriving for a Firing that's admitted but not delivered sends a mail `Cancel`, and the Firing ends as `cancelled`.

**Deliver and run (R7–R10).**
1. The target Router receives the item in its inbox (existing).
2. The injection owner calls `firing_injection`, which inserts the `firing_receipts` row. A duplicate is a no-op.
3. For a Wake: the existing wake push is built and delivered, then `FiringReport(delivered)` is sent.
4. For a Schedule: a `workflow_runs` row is inserted with the `firing_id`, and the run starts through the existing execution route, followed by `FiringReport(runStarted)`. The summary worker runs as today, followed by `FiringReport(runFinished, summary)`, or `failed` with its reason.
5. The mail sender status (`held`, `delivered`, `expired`, `cancelled`) also updates the Firing.

**Hand over (R11–R13).** At first startup on the new release, after enrollment, for each local Schedule and each active or paused Wake:
1. In one local transaction, set `automation_handover = pending`. The local timers now skip it.
2. Call `AutomationImport`. It carries the definition, its `watermark_ms` and `next_due_ms`, and its occupying and waiting run state.
3. The Fleet inserts it, keyed by `automation_id`.
   - A repeat with the same digest gets the same answer.
   - A different digest is refused as `importConflict`, and the Router reports it and keeps the Automation `pending`.
4. On acceptance, set `moved`.
5. The Fleet admits from the imported watermark, so a due time that fell during the move fires once, under the fold rules.
6. A crash between steps resumes at the same step.

An old-release machine never calls Import, so the Fleet holds nothing of its definitions and never fires them.

## Interfaces

- **Calls** (typed request and answer enums, through `CallTarget` on the Fleet face):
  - `AutomationCreate { definition }` → `{ automation_id, revision }`
  - `AutomationChange | Pause | Resume | Cancel { automation_id, expected_revision }` → `{ revision }`, or the refusals `revisionMismatch`, `wakeExists { creator }`, `limitExceeded { limit }`
  - `AutomationList { filter } | AutomationShow { automation_id }` → definitions, state and recent Firing outcomes
  - `AutomationImport { automation_id, definition, timing, run_state }` → `imported | alreadyImported | importConflict`
  - `FiringReport { firing_id, state, summary? }` → `recorded | stale`
- **Mail:** `FiringBody { firing_id, automation_id, revision, due_at, valid_until, payload: Wake { message, creator } | ScheduleRun { instruction, destination, model, effort, timeout } }`.
- **CLI and MCP:** the existing `wake …` and `schedule …` commands keep their flags and JSON shapes. The target may now be on any machine, and each command's answer comes from the Fleet. Offline writes fail with `centralUnreachable`.

## State

- Firing (central): `due` → `held` → `delivered`, then `runStarted`, then `runFinished` or `failed`; and `due`/`held` → `expired` or `cancelled`. It's monotonic, so a stale `FiringReport` is answered `stale`.
- Handover (local): `none` → `pending` → `moved`.

## Runtime behaviour: clock, delivery, scale

- **The clock.** The Fleet object holds one platform alarm, owned by `AlarmOwner` (`runtime-primitives/src/alarm_owner.rs:34-53`); a request only moves it earlier. On Cloudflare, `DurableObject::alarm()` (`object_shell.rs:448`) calls `on_alarm`, which calls `durable_steps::drive_due` (`operation_engine.rs:210`). That reads every due operation in one transaction, re-arms to the next earliest deadline, and runs them with bounded concurrency. There's no polling. On Tokio, `wake_at` is a Tokio timer (`native_objects.rs:65`).
- **Batching.** One alarm admits every Automation due at or before now in one transaction, so N automations due together cost one wake.
- **Delivery to machines.** Each machine holds one hibernatable WebSocket to its Connection object (`object_shell.rs:1`, `binding_attachment.rs:1`). A Firing in recipient custody becomes one journaled notice plus a live hint (`machine_notice_delivery.rs:1`). The machine pulls, and when it's offline it catches up from its position on reconnect.
- **The wake push** is unchanged: `wakeup_delivery_sender` builds the `PushKind::Wake` push, and `session_delivery_router` picks one route (Codex app-server, Claude Code peer, ACP) per attempt. The modes are `auto` (idle starts a turn; busy queues, then one prompt), `queue` and `steer`.
- **Bounds:** one alarm per object; one socket per machine; one active Wake per session; one occupying and one waiting Run per Schedule; an undelivered Wake Firing absorbs new due times.
- **Hot spot:** a single Fleet object holds every Automation. That's adequate for tens to hundreds. If thousands with second-level timing ever appear, round due times to the minute (deferred; not needed now).

## Failure and edge behaviour

| Case | Behaviour |
| --- | --- |
| Central unreachable during a write | refused as `centralUnreachable`, never queued (R4) |
| Central restarts during an alarm | the durable step resumes; the Firing row and its mail obligation are in one transaction, so there's no double admission |
| Mail submit's outcome unknown | reconciled through sender status, never resubmitted with a new identity (R6a) |
| Target machine offline | the item is held, then delivered or expired by `valid_until` (R6) |
| The same item delivered twice | the `firing_receipts` primary key turns the second into a no-op (R7) |
| A FiringReport is lost | the durable outbox retries it; the Fleet answers repeats idempotently |
| Import conflict | stays `pending`, is reported to the operator, and never fires on both sides |

## Trust boundaries

Calls ride the authenticated machine session, so the installation is bound by central. The `creator` is recorded as declared, which is today's wake rule (spec 1 §6). A `FiringReport` is accepted only from the installation the Firing was addressed to.

## Migration of storage

- **Fleet face:** a new migration adds the four tables on both runtimes.
- **Router `automation.sqlite`:** adds `firing_receipts`, `automation_handover`, and a unique `workflow_runs.firing_id`. Existing tables stay, under PR 1's retention, WAL and vacuum.
- The `remote_wake_requests` rows stay as history and are pruned at 2 months.

## Trace table

| Capability | R | Component | Seam (proof) |
| --- | --- | --- | --- |
| definitions | R1 | catalog + automation calls + operations | the List/Show answer from a second Router |
| definitions | R2 | catalog (no project field), clock | Firing at its due time while all leases are expired |
| definitions | R3 | catalog checks | the refusal answers |
| definitions | R4 | operations | the `centralUnreachable` answer; no later insert |
| firing | R5 | clock | `automation_firings` rows; the alarm drive report |
| firing | R6 | firing sender + mail custody | mail status; the recipient's inbox |
| firing | R6a | catalog revision + firing sender cancel | Firing state; no injection |
| firing | R7 | firing injection | `firing_receipts`; the count of pushes and runs |
| runs | R8 | firing injection → wake push | the session's received push |
| runs | R9 | firing injection → scheduled-run route | the `workflow_runs` row; the session |
| runs | R10 | reporter + sender status | Show from a non-target Router |
| migration | R11 | handover | `automation_handover`; Fleet rows |
| migration | R12 | handover + clock watermark | the count of Firings for the due time |
| migration | R13 | handover (absent on old releases) | the Fleet holds no row for the machine |
| migration | R14 | PR 1 retention | local history reads |
| design-truth | R15 | the docs reconciliation process | the published tree, the manifest and the scan |
