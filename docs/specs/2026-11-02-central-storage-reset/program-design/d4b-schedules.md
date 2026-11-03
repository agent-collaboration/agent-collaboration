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

## Lead decisions (strike any that is wrong)

1. **Overlapping runs:** today a schedule will not start a run while its previous one is still running, and folds waiting runs (`admit_waiting_run`). Keeping that needs run state the owner excluded from central (“definitions only”). Decision: drop it. Each occurrence is one message and the Fleet sends only the latest; a slow session receives the next instruction queued.
2. **Run summaries:** today's post-run summaries (`run summaries`, `summary-retry`) are history. Decision: delete them with `workflow_runs`.
