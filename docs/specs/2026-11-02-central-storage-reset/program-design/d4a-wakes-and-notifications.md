# Program Design: D4a, wakes and notifications

How D4a realizes U5, U6 (notification cadence), U14, U15, U16. Builds on D1's Fleet outbox and D2's Fleet sender. Evidence: `tmp/d2-inventory/04-wakes-and-notifications.md`, `05-schedules-and-surfaces.md`.

## What you will be able to do

`agent-collaboration wake send --to <session on any machine> --every 30m --for 6h --text "check the build"` from any machine. The wake lives in the Fleet, so it keeps firing while the machine that set it is off. Following a board thread gives you one listing of new activity at most every 10 minutes. No Router keeps wakes or notification timers.

## Wakes: definitions only, firing from the definition

`automation_wakes` (Fleet store) holds only what the owner or an agent defined:

| Column | Meaning |
| --- | --- |
| `id` UUIDv7 PK, `created_at`, `updated_at` | |
| `created_by_*` | the creating session (machine, endpoint, session id) |
| `target_machine_id`, `target_endpoint`, `target_session_id` | who receives it |
| `message_text`, `delivery_mode` | what is sent |
| `timing_kind` (`once`, `every`, `cron`) with `first_due_at`, `every_seconds`, `cron_expression`, `cron_timezone` | when (validated in Rust: times on the 5-minute grid, off-grid refused; repeats at least 10 minutes apart; today's lifetime limits kept: once at most 24 h ahead, repeating at most 7 days) |
| `expires_at` | required: every wake has a time limit |
| `paused` (boolean), `cancelled_at` | the owner's controls |

There is **no firing table and no next-due table.** On each alarm the Fleet computes, from the definition alone, the latest due occurrence not later than now. The occurrence's message id is a UUIDv7 whose time part is the due time and whose random part is derived from the wake id, so a second attempt at the same occurrence is the same message: D1's outbox stores it once (`outbox_admission.rs:177–183`). A machine that was offline gets the **latest** occurrence only, not every missed one. A wake counts as **fired** when that message's status reaches “in recipient's inbox” (U15), read from the outbox item, which D1 keeps for 60 days. Pausing, changing or cancelling affects only occurrences not yet sent; nothing chases a sent message.

Today's local wakes (`automation.sqlite` `wakeup_definitions`, `mailbox_deliveries`, `remote_wake_requests`, the 250 ms `WakeTimingWorker`, the remote-wake mail path) are deleted. Nothing is migrated; agents set their wakes again.

## Notifications as wakes

Today the **project's writer machine** scans its local project store and sends notifications as project mail (`subscription_windows.rs`, follow admission, `intent_handoff`), with its own quiet/cap timers. D4a moves this to the Fleet:

- One notification pass per subscriber machine every 10 minutes, offset by a fixed per-machine jitter of 0–60 seconds derived from the machine id.
- The pass reads each of that machine's subscriptions (subscription memberships are project content) and the activity since its last coverage from the **project objects** in central (D1's Fleet → project read; no Fleet transaction held across it). It sends one listing per subscriber as a Fleet message, or nothing if there is no new activity.
- Coverage is kept in `notification_coverage` (Fleet store): `id` UUIDv7 PK, `membership_id` UNIQUE, `covered_through` (project feed position), `created_at`, `updated_at`. This is delivery state, not an automation definition, so it is not an `automation_` table.
- The Router's notification producer and timers (`subscription_windows`, `reader_delivery_owner` timers, `notification_*` progress tables) are deleted; notifications reach sessions only through the inbox.

## Surfaces

`wake send | show | list | pause | resume | cancel` keep their names and talk to the Fleet. `--wait-until-first-fire` waits for the occurrence's inbox status. Limits are in the JSON schema, the CLI help and the MCP tool schemas, and a refusal names the limit (U16). Delivery-attempt commands that read `mailbox_deliveries` are deleted.

## Decisions for the owner

1. **A target that was offline across several firings:** (a) gets only the latest, and older unsent ones are superseded (needs explicit supersession of queued items, not just “compute the latest”); or (b) gets every one. Recommended: (a), because a wake is a reminder and repeats add nothing.
2. **Notification coverage:** the Fleet keeps one coverage record per subscription (`notification_coverage`) so each listing knows what it already covered. It is delivery state, not an automation definition. Recommended: keep it; without it a restart can miss or repeat activity.

## Occurrences, pause, cancel and first fire

A wake has no edit today (`WakeAction` is pause, resume, cancel: `wakeup_mutations.rs:11–15`, `:77–121`), and D4a adds none: changing a wake means cancelling it and setting a new one.

`automation_wakes` gains four columns that record the wake's own state, not a history: `last_occurrence_due_at`, `last_occurrence_item_id` (the Fleet outbox item of the latest admitted occurrence), `first_fired_at`, `first_fire_item_id` (set once, never cleared).

| Event | What happens, in one Fleet transaction |
| --- | --- |
| Alarm, an occurrence is due | Compute the latest due time not later than now and before `expires_at`. Admit its message to the Fleet outbox under the occurrence id (UUIDv7: time part = due time, random part derived from wake id and due time). If the previous occurrence is still before “in recipient's inbox”, cancel it through D1's cancel (prevented if not yet pushed; too late otherwise): this is decision 1 (a). Record `last_occurrence_*`. A crash before commit replays to the same id; D1's outbox stores it once |
| Pause | `paused = true`; cancel the queued occurrence if it has not reached the recipient's inbox, as today pause discards pending deliveries (`wakeup_mutations.rs:123–130`) |
| Resume | next due is computed from the resume time; paused ticks are skipped, as today (`wakeup_mutations.rs:87–112`); a wake with no due time left finishes |
| Cancel | `cancelled_at` set; queued occurrence cancelled as for pause |
| Expiry | no occurrence is admitted at or after `expires_at`; a queued one still undelivered at expiry is cancelled |
| Recipient's inbox accepts an occurrence | the Fleet's status observer (D2's completion observer, the same code) sets `first_fired_at` and `first_fire_item_id` if empty; later occurrences never overwrite them |

An admitted occurrence's message is frozen (its digest is in the outbox item). Words used on every surface: **sent** = admitted to the Fleet outbox; **fired** = accepted into the target's inbox. `--wait-until-first-fire` reads `first_fired_at`.

## Notification passes

**Decision (Lead, reversible; owner words “it should be done with wakes”):** a notification pass is a wake occurrence of a second kind, run by the same Fleet alarm, with the same occurrence-id rule and the same Fleet-message path. Its schedule is derived, not stored: every subscriber machine enrolled in the Fleet gets one pass every 10 minutes, offset by 0–60 s from its machine id, so there is no system row in `automation_wakes` for the owner to see or pause. The Advisor's alternative (an `action_kind` column on `automation_wakes`) would put Fleet-owned rows beside owner-defined ones with different limits; deriving the schedule from enrollment avoids that.

`notification_coverage` (Fleet store), one row per subscription membership:

| Column | Meaning |
| --- | --- |
| `id` UUIDv7 PK, `created_at`, `updated_at` | |
| `project_id`, `membership_id` (UNIQUE) | which subscription |
| `membership_generation`, `activated_epoch`, `activated_position` | which life of the subscription this coverage belongs to (today's `generation`, `activated_at`: `subscription_delivery_progress.rs:42–49`) |
| `covered_epoch`, `covered_position` | everything in the project feed up to here has been listed or deliberately skipped (`FeedCursor`: `record_positions.rs:101–104`) |
| `last_listing_item_id` | the Fleet message of the latest listing, if any |

Today's progress keeps a cursor per thread because each thread has its own quiet and cap timers (`subscription_delivery_progress.rs:34–38`). One pass lists every eligible thread at once, so one cursor per membership is enough.

**One pass for machine M:**

1. Read M's memberships and, per membership, the project feed after `covered` from the project objects (D1's Fleet → project read; no Fleet transaction is open during the read).
2. Apply today's filters (own posts, thread Off and Poll overrides, Off memberships: `subscription_window_scan.rs:175–214`). Build one listing per subscribing session: per thread, the range (from, through), post count and read context, as structured fields, not prose.
3. One Fleet transaction: check each coverage row still equals the cursor the read started from (otherwise drop this pass; the next one re-reads); admit the listing under its id (UUIDv7: pass due time + membership); advance `covered` to the scan head. If admission is refused, the transaction rolls back and the range is not consumed. With no new activity, or only filtered activity, coverage advances without a listing.
4. Generation change (unsubscribe and subscribe again): coverage restarts at the new activation. Epoch change (project reset): coverage restarts at the new epoch's head; old-epoch activity is not listed.

## What stays on the Router

Only notification production and its timers go: `subscription_windows`, `subscription_window_scan`, the `reader_delivery_owner` quiet/cap timers, the `notification_*` progress tables and the board intent publisher. Native mail delivery, held and poll handling in `reader_delivery_owner` stay (`mail_delivery_runtime.rs:583`): a listing arrives as an ordinary Fleet message in the inbox.
