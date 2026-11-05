# Requirements: local-storage

Each Router's tables carry one prefix per concern, and JSON is kept only with a stated reason.

**Today.** Table names mix location, encoding and concern words: `router_pushes`, `local_delivery_retries`, `pending_poll_mail`, `typed_interaction_history`, `board_intent_progress` (`crates/interaction-storage/`). Router state also lives in JSON files such as `machine-session/state.json`, `machine-notice-progress.json` and `automation-settings.json`.

| ID | Need | Owner's words | Authority |
| --- | --- | --- | --- |
| <a id="u11"></a>U11 | Every table we own starts with one prefix naming its concern. Approval and question tables are renamed too; their behavior does not change. The library's `_fleet_sync_` names stay | “making sure all tables have good prefix_ and don't do slop”; “make sure all automation tables have automation_ prefix”; no to L14 (“rename them too”); keep on L10 | authorized, 2026-11-02 |
| <a id="u12"></a>U12 | JSON files are cleaned up and consolidated; JSON is kept only where there is a stated reason | “we need to clean up json and consolidate and think about what should be json” | authorized, 2026-11-02 |
| <a id="u13"></a>U13 | SQL CHECK constraints are used only for booleans | “our restriction of checks to stop enums is good. only check is for boolean” (as recorded in the 2026-11-02 handoff); `AGENTS.md` SQLite section | authorized |

**Also serves:** U11 also serves `central-layout` and `automations` (their tables follow the same rule).

**Not asked for:** moving approvals or questions out of `interaction.sqlite`; changing their retention.
