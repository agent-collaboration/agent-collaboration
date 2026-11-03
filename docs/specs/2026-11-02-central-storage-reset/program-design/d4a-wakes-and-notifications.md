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
| `paused_at`, `resumed_at`, `cancelled_at` | the owner's controls; `resumed_at` is the lower bound for occurrences after a resume |

`automation_wakes` holds definitions only (U14: “i dont want automation tables in central to have anything other than definitions of automations”). What happened to each occurrence is delivery state and lives beside the Fleet outbox, in one small row per wake, `fleet_wake_state` (below). On each alarm the Fleet computes, from the definition, the latest due occurrence not later than now. Its message id is a UUIDv7 whose time part is the due time and whose random part is derived from the wake id, so a second attempt at the same occurrence is the same message: D1's outbox stores it once (`outbox_admission.rs:177–183`). A wake counts as **fired** when that message reaches the recipient's inbox (U15). Pausing or cancelling affects only occurrences not yet sent; nothing chases a sent message (U15: “we do not have to chase to the machine for cancel”).

Today's local wakes (`automation.sqlite` `wakeup_definitions`, `mailbox_deliveries`, `remote_wake_requests`, the 250 ms `WakeTimingWorker`, the remote-wake mail path) are deleted. Nothing is migrated; agents set their wakes again.

## Notifications as wakes

Today the **project's writer machine** scans its local project store and sends notifications as project mail (`subscription_windows.rs`, follow admission, `intent_handoff`), with its own quiet/cap timers. D4a moves this to the Fleet:

- One notification pass per subscriber machine every 10 minutes, offset by a fixed per-machine jitter of 0–60 seconds derived from the machine id.
- The pass reads each of that machine's subscriptions (subscription memberships are project content) and the activity since its last coverage from the **project objects** in central (D1's Fleet → project read; no Fleet transaction held across it). It sends one listing per subscriber as a Fleet message, or nothing if there is no new activity.
- Coverage is kept in `notification_coverage` (Fleet store, delivery state, not an `automation_` table; columns below).
- The Router's notification producer and timers (`subscription_windows`, `reader_delivery_owner` timers, `notification_*` progress tables) are deleted; notifications reach sessions only through the inbox.

## Surfaces

`wake send | show | list | pause | resume | cancel` keep their names and talk to the Fleet. `--wait-until-first-fire` waits for the occurrence's inbox status. Limits are in the JSON schema, the CLI help and the MCP tool schemas, and a refusal names the limit (U16). Delivery-attempt commands that read `mailbox_deliveries` are deleted.

## Decisions for the owner

1. **A target that was offline across several firings:** (a) gets only the latest, best effort: an older occurrence that was never pushed is cancelled, one already pushed is not chased; or (b) gets every one. Recommended: (a), because a wake is a reminder and repeats add nothing.
2. **Notification coverage:** the Fleet keeps one coverage record per subscription (`notification_coverage`) so each listing knows what it already covered. It is delivery state, not an automation definition. Recommended: keep it; without it a restart can miss or repeat activity.

## Occurrences, pause, cancel and first fire

A wake has no edit today (`WakeAction` is pause, resume, cancel: `wakeup_mutations.rs:11–15`, `:77–121`), and D4a adds none: changing a wake means cancelling it and setting a new one.

`fleet_wake_state` (Fleet store, not an `automation_` table): `id` UUIDv7 PK (= the wake id), `pending_occurrence_id` (nullable: the latest admitted occurrence not yet pushed), `first_fired_at` (nullable), `first_fire_item_id` (nullable), `created_at`, `updated_at`. One row per wake, holding only the pending pointer and the first-fire receipt U15 asks for; it is removed with the wake's definition. There is **no per-occurrence row in central**: an occurrence's id is computed from the wake id and its due time, and D1's outbox answers whether an id exists, was pushed, or reached the inbox. Computed ids alone cannot say which earlier occurrence is still queued after downtime, so `pending_occurrence_id` names it: it is set in the transaction that admits an occurrence and cleared when the status observer sees that item pushed.

**Sent** means admitted to the Fleet outbox; **fired** means accepted into the target's inbox. These words are the same on every surface.

| Event | What happens, in one Fleet transaction |
| --- | --- |
| Alarm | Compute the latest due time ≤ now, ≥ `resumed_at`, < `expires_at`. Admit its message under the computed id. If the outbox already holds that id, admission answers `replayed` and nothing else happens: another alarm in the same period already sent it, and nothing is cancelled. A crash before commit replays to the same id |
| Older occurrence still queued (decision 1 (a) only) | Before admitting the new one, cancel the item named by `pending_occurrence_id` **only if it has never been pushed** (D1's definitely-unsent cancellation, `outbox_admission.rs:129`). One already pushed stays as it is. So “latest only” is best effort: a target that was briefly reachable can receive two |
| Pause, cancel | set `paused_at` or `cancelled_at`; cancel the item named by `pending_occurrence_id` if it has never been pushed, and clear the pointer |
| Resume | set `resumed_at`; ticks due before it are never admitted, as today (`wakeup_mutations.rs:87–112`); a wake with no due time left finishes |
| Expiry | nothing is admitted at or after `expires_at`; a never-pushed occurrence is cancelled |
| Target inbox accepts an occurrence | the Fleet's status observer (the same observer as D2's) sets `first_fired_at` and `first_fire_item_id` if they are empty |

**Proof cases:** two alarms in the same period (one message, nothing cancelled); occurrence t0 admitted and never pushed, the Fleet down across t1–t4, restart at t5 (t0 cancelled, only t5 delivered); pause after the same downtime (t0 cancelled); pause in one period and resume two periods later (no paused tick is sent); the target's receipt lost while a newer occurrence supersedes (no state goes backwards; the pushed one is not cancelled). For notifications: a second pass in the same slot after a new membership appears (no second listing under the id); a pass whose activity exceeds the root cap (the rest arrives next slot, nothing skipped); a superseded epoch cursor (resumes from the project's position).

## Notification passes

**Lead choice (reversible), one scheduler:** a notification pass runs from the same Fleet alarm as wakes and sends a Fleet message, as the owner put it (“it should be done with wakes”). Its schedule is derived, not stored: every enrolled machine gets one pass every 10 minutes, offset by 0–60 s from its machine id. So there is no Fleet-owned row in `automation_wakes` beside the owner's.

`notification_coverage` (Fleet store, delivery state), one row per subscription membership:

| Column | Meaning |
| --- | --- |
| `id` UUIDv7 PK, `created_at`, `updated_at` | |
| `project_id`, `membership_id` (UNIQUE) | which subscription |
| `membership_generation`, `activated_epoch`, `activated_position` | which life of the subscription this row belongs to (today's `generation`, `activated_at`: `subscription_delivery_progress.rs:42–49`) |
| `covered_epoch`, `covered_position` | everything in the feed up to here is listed or deliberately filtered (`FeedCursor`: `record_positions.rs:101–104`) |
| `last_listing_item_id` | the latest listing that advanced this row |

**One pass for machine M, slot S:**

1. **Fence per machine and slot.** `notification_pass_fence` (Fleet store): `id` UUIDv7 PK (= the machine id), `last_slot_due_at`, `created_at`, `updated_at`. If `last_slot_due_at` ≥ S, stop: this slot already ran for the whole machine, and every listing it produced is frozen in the outbox. A membership that appears, or becomes readable, after its slot ran waits for the next slot; nothing is ever recomputed under an admitted listing id.
2. Read each remaining membership's feed after `covered` from the project objects (D1's Fleet → project read; no Fleet transaction is open during the read). Classify the stored cursor with today's lineage rule (`project_epoch_reads.rs:266–329`, `subscription_window_lineage.rs:49–78`): **valid** reads on; **superseded** resumes from the position the project gives; **invalid** is reported, and coverage does not jump. An epoch change is never by itself a reason to skip activity.
3. Apply today's filters (own posts, thread Off and Poll overrides, Off memberships: `subscription_window_scan.rs:175–214`) and today's size caps (`subscription_window_selection.rs:110–126`, `thread_subscriptions.rs:25`). Take activity in feed order until a cap is reached. The listing covers through the last included or filtered position; the rest waits for the next pass.
4. Build one listing per subscribing session. Its id is a UUIDv7: time part S, random part derived from (M, session). Per thread it carries the range, the post count and the read context, as structured fields.
5. One Fleet transaction for the whole pass: check the fence is still below S and each row's (generation, activation, covered cursor) still matches what step 2 read; admit every listing of the pass; advance each row's `covered` and `last_listing_item_id`; set the fence to S. If admission is refused, the transaction rolls back and nothing is consumed. With only filtered activity, coverage advances without a listing.

A generation change (unsubscribe and subscribe again) starts a new row life at the new activation.

## What stays on the Router

Only notification production and its timers go: `subscription_windows`, `subscription_window_scan`, the `reader_delivery_owner` quiet/cap timers, the `notification_*` progress tables and the board intent publisher. Native mail delivery, held and poll handling in `reader_delivery_owner` stay (`mail_delivery_runtime.rs:583`): a listing arrives as an ordinary Fleet message in the inbox.

**Status: not ready for planning** until the owner's decisions above are made and the Advisor's second look at this revision is closed.
