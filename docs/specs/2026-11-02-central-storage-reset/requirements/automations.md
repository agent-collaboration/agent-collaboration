# Requirements: automations

Wakes and schedules are definitions held only in central and fired by Fleet as messages; notifications are wakes.

**Today.** Each Router keeps its own automations in `automation.sqlite` (12 tables, `crates/automation-storage/migrations/`) with local timer workers. A repeating wake may fire every 5 minutes (`crates/automation-storage/src/wake_limits.rs:9`); a one-shot wake is limited to 24 hours ahead and a repeating one to 7 days (`wake_limits.rs:7–8`). Board notifications run their own separate timers (`crates/collaboration-service/src/subscription_delivery/subscription_windows.rs:505–515`).

Quotes dated before 2026-11-02 are as recorded in that day's v2 requirements file; the owner may re-confirm them.

| ID | Need | Owner's words | Authority |
| --- | --- | --- | --- |
| <a id="u14"></a>U14 | Automations are definitions of message schedules, held only in central and shared by every machine. Central automation tables hold definitions only and start `automation_`. No Router has automations. Nothing is migrated: agents set their wakes again, and old history is deleted | “automations are basically definitions of message schedules. routing messages”; “it was supposed to be cross system in a central location”; “i dont want automation tables in central to have anything other than definitions of automations”; “yes all routers dont have automation”; “no moves we just message the agents manually to update their wakes”; “move automation to central only no migration at all, wakes too” | authorized |
| <a id="u15"></a>U15 | Fleet fires an automation as an ordinary message. A wake counts as fired once its message is accepted into the target machine's inbox. Changing or cancelling one affects only what has not been sent | “central has to send automated messages”; “acceptance to inbox is good”; “we do not have to chase to the machine for cancel” | authorized |
| <a id="u16"></a>U16 | Wakes fire at most every 10 minutes and every wake has a time limit. Times are on a 5-minute grid, and off-grid times are refused. The limits are in the schema, the CLI and MCP help, and validation. A subscription's notifications are delivered by a wake | “wakes should be 10min min”; “all wakes should have a time limit”; “app only allows 5 min precision … not fake precision”; “the product contracts should be part of schema and max validation too and understood by agents”; “it should be done with wakes i guess wakes are restricted in the system only every 10min max” | authorized |
| <a id="u17"></a>U17 | Schedules move to central after wakes and notifications | “if you feel like it's another deliverable for notifications and wakes that's fine then we do automation” | authorized, 2026-11-02 |

**Not asked for:** per-firing history in central; migrating existing wakes or schedules; automations tied to a project.
