# Program Design: D4b, schedules

How D4b realizes U14, U15, U17 for schedules, after D4a. Evidence: `tmp/d2-inventory/05-schedules-and-surfaces.md`.

## What you will be able to do

Create a schedule from any machine: an instruction, a time pattern, and a destination (an existing session, or a fresh session on a named machine and folder with model and effort). The Fleet runs it at its time; a fresh-session schedule uses D2's create command with the instruction as first input. `automation.sqlite` and every local automation timer are gone.

## Definitions (Fleet store)

| Table | Columns | Replaces |
| --- | --- | --- |
| `automation_schedules` | `id` UUIDv7 PK, `created_by_*`, `instruction_text`, `instruction_revision` (UUIDv7, for “update only if unchanged”), timing as in `automation_wakes`, `expires_at` (required or optional per decision 2), `destination_kind` (`bound`, `fresh`), bound: `target_machine_id`, `target_endpoint`, `target_session_id`; fresh: `target_machine_id`, `endpoint`, `working_directory`, `model`, `effort`, `access`; `timeout_seconds`, `enabled` (boolean), `created_at`, `updated_at` | `schedule_definitions`, `instruction_documents`, `thread_bindings` |

Deleted, with no central counterpart: `instruction_revisions` (history), `schedule_timing_state`, `workflow_runs` (the run holding the slot moves to `fleet_schedule_state`, its route evidence to `mail_schedule_runs` on the target), `operation_receipts`, `automation_events`, `owed_push_operations`, `automation-settings.json`.

## Firing

Same rule as wakes: the occurrence id is a UUIDv7 from the due time and the schedule id, so a second attempt is the same message; the latest occurrence only after an outage. A bound schedule sends the instruction as a Fleet message to its session. A fresh schedule sends D2's create command with the instruction as first input; the run's result is that command's report. A bound session is held by at most one schedule (today's `thread_bindings` rule becomes a UNIQUE on the bound target).

## Decisions for the owner

1. **Overlapping runs:** today a schedule does not start a run while its previous one is still running, and folds waiting runs (`run_admission.rs:39–46`). (a) Keep that, with the run owned and reported by the target machine (below); or (b) drop it, so each due time sends regardless. Recommended: (a); (b) can queue repeated work or start many fresh sessions while one is still busy.
2. **Schedule lifetime:** schedules do not expire today (`schedule_definition.rs:38–47`); the 24-hour and 7-day limits are wake rules. Recommended: schedules keep no forced expiry; an optional `expires_at` only.
3. **Continuity between runs:** fresh-session runs today carry the previous run's summary into the next (`run_admission.rs:56–68`). (a) Keep the latest continuity as part of the definition; or (b) every fresh run starts with no context. Recommended: (a), because dropping it drops working context.

## How a run starts, ends and is recorded

**The Fleet cannot see a model turn, and the target machine sees it only through the route it used.** D1's report says a message reached the session (`delivered`), and D2's result says a first input was `accepted`; neither says the turn finished. Today the Router that runs the schedule keeps the route's effect evidence and watches the run through it (`scheduled_run_worker.rs:170–188`), and at the deadline it asks that route to stop (`:189–213`). Not every route can see completion: a Claude Code terminal peer reports only “written, completion unknown” and has no summary (`claude_code_peer_scheduled_runs.rs:275–296`).

So the run is owned on the **target machine B**, through today's seam, and the Fleet keeps only its outcome. `automation_schedules` stays definitions only (U14).

| Table | Store | Columns | Job |
| --- | --- | --- | --- |
| `mail_schedule_runs` | B (`interaction.sqlite`) | `id` UUIDv7 PK (= occurrence id), `schedule_id`, `deadline_at`, route effect evidence (typed per route, as today's `RouteEffectEvidence`), `outcome` (`pending`, `completed`, `failed`, `interrupted`, `stopped_at_deadline`, `written_without_completion`, `unknown`), `summary` (nullable), `report_owed` (boolean), `created_at`, `updated_at` | binds the occurrence to the one native request or turn it caused; `report_owed` survives restart until the Fleet acknowledges the report |
| `fleet_schedule_state` | Fleet | `id` UUIDv7 PK (= the schedule id); the run that holds the slot: `current_occurrence_id`, `current_due_at`, `current_status` (`sent`, `at_recipient`, `running`, `unknown`, or null when free), `current_session_*`; the continuity in use: `continuity_occurrence_id`, `continuity_due_at`, `continuity_summary`; `created_at`, `updated_at` | one row per schedule, removed with its definition. It is current state only: no row per run, no outcome history. Each run's final outcome is in its mail item and B's report, both under D1's retention |

**Run ended (machine socket call, recipient only):** `schedule run ended { occurrence_id, outcome, summary? }`. Only the machine that received the occurrence may send it (D1's rule for reports). The Fleet applies it only if `occurrence_id` equals `current_occurrence_id`. A report for an older occurrence is acknowledged and changes nothing except, under the continuity rule below, the continuity. A repeat answers the same. B sends it from `mail_schedule_runs` when the outcome becomes final and retries until it is acknowledged.

**Deadline:** the occurrence carries `deadline_at` (sent time + `timeout_seconds`). B asks the route to stop at the deadline, as today. B reports `stopped_at_deadline` only when the route confirms the stop (native history shows the turn interrupted, as `run_reconciliation.rs:98–104` checks); a stop request alone stays `pending`. The Fleet sets no timer of its own and never frees a slot by itself.

**Overlap (decision 1 (a)), keeping today's occupancy rule** (`run_lifecycle.rs:20–31`): the slot is busy while `current_status` is `sent`, `at_recipient`, `running` or `unknown`. `unknown` occupies, as today's `Uncertain` does. `written_without_completion` frees the slot, as today a peer-written run goes straight to finished with no summary (`run_completion_state.rs:71–89`). Due times that pass while the slot is busy fold into one (`run_admission.rs:39–46`). The slot lives in the schedule's own row, so mail retention never frees it. If B never comes back, or a run stays `unknown`, the schedule waits; `schedule show` says why, and the owner can clear the slot or disable the schedule. That is today's behaviour for an uncertain run, not a new recovery path.

**Continuity (decision 3 (a)), frozen at admission:** when a fresh run is admitted, its create command carries the current `continuity_summary`, and the outbox digest freezes it (the same job `run_admission.rs:69–90` does for today's captured inputs). A completed report with a summary replaces the continuity only if its due time is later than `continuity_due_at`. So a late report from an older run can never change what a newer run was given, before or after a restart.

**Fresh-session runs use D2 from inside the Fleet:** the Fleet admits a `fleet_remote_creates` row itself, with `requester_kind = schedule` and `schedule_id` (two new columns; the `requester_*` session columns are null for this kind only). No machine is impersonated. D2's result is matched to the occurrence through `command_item_id` and applied to `fleet_schedule_state` under the same rule as a run-ended report. It is not sent as a message to the schedule's creator, who did not ask for one.

**Status: not ready for planning** until the owner's decisions above are made and the Advisor's second look at this revision is closed.
