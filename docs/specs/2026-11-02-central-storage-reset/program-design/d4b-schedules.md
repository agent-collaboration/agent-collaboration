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

1. **Overlapping runs:** today a schedule does not start a run while its previous one is still running, and folds waiting runs (`run_admission.rs:42–90`). (a) Keep that, tracking the running occurrence through its mail or create command (no history table); or (b) drop it, so each due time sends regardless. Recommended: (a); (b) can queue repeated work or start many fresh sessions while one is still busy.
2. **Schedule lifetime:** schedules do not expire today (`schedule_definition.rs:38–47`); the 24-hour and 7-day limits are wake rules. Recommended: schedules keep no forced expiry; an optional `expires_at` only.
3. **Continuity between runs:** fresh-session runs today carry the previous run's summary into the next (`run_admission.rs:56–79`). (a) Keep the latest continuity as part of the definition; or (b) every fresh run starts with no context. Recommended: (a), because dropping it drops working context.

## Design work remaining before D4b is planned

- **Internal create path:** a schedule fires while its creator may be offline, so the Fleet needs an internal producer path into D2's request ledger (`fleet_remote_creates`) with the schedule as provenance; no impersonation of a connected machine and no unrequested messages to the creator.
- **Timeout:** define what `timeout_seconds` bounds on the mail and D2 path (an accepted message is not a finished turn).
- **Run outcome:** D2's “first input accepted” is not model-turn completion; say what a schedule records as a run's end, if anything.
