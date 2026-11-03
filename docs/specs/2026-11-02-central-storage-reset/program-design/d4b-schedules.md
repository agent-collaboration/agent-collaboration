# Program Design: D4b, schedules

How D4b realizes U14, U15, U17 for schedules, after D4a. Evidence: `tmp/d2-inventory/05-schedules-and-surfaces.md`.

## What you will be able to do

Create a schedule from any machine: an instruction, a time pattern, and a destination (an existing session, or a fresh session on a named machine and folder with model and effort). The Fleet runs it at its time; a fresh-session schedule uses D2's create command with the instruction as first input. `automation.sqlite` and every local automation timer are gone.

## Definitions (Fleet store)

| Table | Columns | Replaces |
| --- | --- | --- |
| `automation_schedules` | `id` UUIDv7 PK, `created_by_*`, `instruction_text`, `instruction_revision` (UUIDv7, for “update only if unchanged”), timing as in `automation_wakes`, `expires_at` (required), `destination_kind` (`bound`, `fresh`), bound: `target_machine_id`, `target_endpoint`, `target_session_id`; fresh: `target_machine_id`, `endpoint`, `working_directory`, `model`, `effort`, `access`; `timeout_seconds`, `enabled` (boolean), `created_at`, `updated_at` | `schedule_definitions`, `instruction_documents`, `thread_bindings` |

Deleted, with no central counterpart: `instruction_revisions` (history), `schedule_timing_state`, `workflow_runs`, `operation_receipts`, `automation_events`, `owed_push_operations`, `automation-settings.json`.

## Firing

Same rule as wakes: the occurrence id is a UUIDv7 from the due time and the schedule id, so a second attempt is the same message; the latest occurrence only after an outage. A bound schedule sends the instruction as a Fleet message to its session. A fresh schedule sends D2's create command with the instruction as first input; the run's result is that command's report. A bound session is held by at most one schedule (today's `thread_bindings` rule becomes a UNIQUE on the bound target).

## Decisions for the owner

1. **Overlapping runs:** today a schedule does not start a run while its previous one is still running, and folds waiting runs (`run_admission.rs:39–46`). (a) Keep that, tracking the running occurrence through its mail or create command (no history table); or (b) drop it, so each due time sends regardless. Recommended: (a); (b) can queue repeated work or start many fresh sessions while one is still busy.
2. **Schedule lifetime:** schedules do not expire today (`schedule_definition.rs:38–47`); the 24-hour and 7-day limits are wake rules. Recommended: schedules keep no forced expiry; an optional `expires_at` only.
3. **Continuity between runs:** fresh-session runs today carry the previous run's summary into the next (`run_admission.rs:56–68`). (a) Keep the latest continuity as part of the definition; or (b) every fresh run starts with no context. Recommended: (a), because dropping it drops working context.

## How a run starts, ends and is recorded

**The Fleet cannot see a model turn.** D1's report says a message reached the session (`delivered`), and D2's result says a first input was `accepted`; neither says the turn finished. Today the run's end is seen locally, because the run and the session are on the same Router (`run_admission.rs:39–46` blocks on an occupying run). In the Fleet, only the target machine can see it, so the target Router tells the Fleet.

**Run ended (new machine socket call, recipient only):** `schedule run ended { occurrence_id, outcome: completed | failed | interrupted, summary? }`. Only the machine that received the occurrence may send it (the same rule as D1 reports); a repeat with the same content answers from the stored outcome; different content is refused. The target Router sends it when the turn that the occurrence started ends. It is a call, not mail, because it updates the Fleet's own schedule row and nobody else reads it.

**Fresh-session runs use D2 from inside the Fleet:** the Fleet admits a `fleet_remote_creates` row itself, with `requester_kind = schedule` and `schedule_id` (two new columns; `requester_*` session columns become nullable for this kind only). No machine is impersonated, and the result is written to the schedule row, not sent as a message to the schedule's creator, who did not ask for one.

`automation_schedules` gains the schedule's own run state (one row, no history):

| Column | Meaning |
| --- | --- |
| `last_occurrence_due_at`, `last_occurrence_item_id` | the latest run's message or create command |
| `last_run_status` | `sent`, `at_recipient`, `started`, `completed`, `failed`, `interrupted`, `timed_out`, `refused`, `expired` (Rust enum, decoded and rejected if unknown) |
| `last_run_session_*` | the session the run used (the bound one, or the fresh one from D2's result) |
| `continuity_text` | the latest run's summary, carried into the next fresh run (decision 3 (a)) |

**Timeout:** `timeout_seconds` bounds the time from the occurrence being sent to “run ended”. When it passes, the Fleet records `timed_out` and the schedule may run again; it does not interrupt the turn, because the Fleet has no interrupt path (a remote interrupt would be a new command like D2's, outside D4b).

**Overlap (decision 1 (a)):** an occurrence is not sent while `last_run_status` is `sent`, `at_recipient` or `started`; due times that pass meanwhile fold into one, which runs when the slot frees (today's folding, `run_admission.rs:39–46`). With (b), the run-ended call is still useful for `last_run_status` and continuity, but nothing waits on it.
