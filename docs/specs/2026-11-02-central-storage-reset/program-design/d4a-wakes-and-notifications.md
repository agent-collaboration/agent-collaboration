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

## Design work remaining before D4a is planned (from the Advisor's second look)

- **Occurrences and edits:** when an edit, pause, resume or cancel takes effect; an occurrence's message is frozen once admitted; resume skips paused ticks as today (`wakeup_mutations.rs:86–98`); alarm replay is atomic with admission; queued items are bounded by the wake's expiry; “sent” means admitted to the Fleet outbox, “fired” means accepted into the target inbox; first-fire evidence survives later occurrences.
- **Listing and coverage commit together:** after reading the project objects, the Fleet freezes the listing under a stable id and, in one transaction, admits it and advances coverage; a refused listing does not consume its range; empty or filtered activity advances coverage deliberately.
- **Coverage identity:** coverage records the project's feed cursor (epoch and position, `record_positions.rs:101–104`), the membership's generation and activation, and the selected ranges, as today's progress does (`subscription_delivery_progress.rs:14–66`); today's filters (own posts, thread overrides, Off and Poll modes) are kept.
- **The pass is a wake:** `automation_wakes` gains an action kind (`message` or `notification pass for machine X`) so both share one scheduler; no second timer system.
- **What stays on the Router:** only the notification timers go; native mail delivery, held and poll handling in `reader_delivery_owner` stay (`mail_delivery_runtime.rs:583`); listings carry structured ranges and read context, not plain text.
