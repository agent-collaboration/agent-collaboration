# Router database retention and space recovery

The standard history window is **two calendar months**, measured with calendar-month arithmetic. Current state and pending work remain available. The proxy's previous-response affinity is the explicit 30-day exception.

## SQLite file modes

| Store | Open path owner | Journal mode | `auto_vacuum` | One-time conversion or recovery |
|---|---|---|---|---|
| `state.sqlite` | [`codex-router-state`](../../crates/codex-router-state/src/sqlite.rs) | WAL | Incremental (`2`) on new and converted files | Existing files are checked on open and converted by the shared maintenance crate when needed. |
| `session-registry.sqlite` | [`lifecycle-observation`](../../crates/lifecycle-observation/src/observation_journal.rs) | WAL | Incremental (`2`) on new and converted files | Existing files are checked on open and converted by the shared maintenance crate when needed. |
| `provider-operations.sqlite` | [`collaboration-runtime`](../../crates/collaboration-runtime/src/provider_operation_store.rs) | WAL | Incremental (`2`) on new and converted files | Existing files are checked on open and converted by the shared maintenance crate when needed. |
| `interaction.sqlite` | [`interaction-storage`](../../crates/interaction-storage/src/interaction_history_database.rs) | WAL, explicitly selected by the opener | Not enabled by Router; the pinned Turso path has no Router `auto_vacuum` setup | The pinned Turso API does not expose the Router's native conversion or `VACUUM` path. |

## Retention policy

| Store | Table or record kind | Window | Prune key or rule | Owner | Named constant |
|---|---|---|---|---|---|
| `state.sqlite` | `previous_response_affinity_owners` | 30 days from creation | `created_unix_seconds` | [`codex-router-proxy` maintenance actor](../../crates/codex-router-proxy/src/maintenance_actor.rs) using `codex-router-state` | `PREVIOUS_RESPONSE_AFFINITY_OWNER_RETENTION_SECONDS` |
| `state.sqlite` | `session_account_affinities` | 1 week after last observation | `last_seen_unix_seconds` | [`codex-router-proxy` maintenance actor](../../crates/codex-router-proxy/src/maintenance_actor.rs) | `SESSION_ACCOUNT_AFFINITY_RETENTION_SECONDS` |
| `state.sqlite` | `quota_history_observations` | 1 week | `observed_unix_seconds` | [`codex-router-state`](../../crates/codex-router-state/src/sqlite/quota_history.rs) through proxy maintenance | `QUOTA_HISTORY_OBSERVATION_RETENTION_SECONDS` |
| `state.sqlite` | Stale `active_client_leases` | 10 minutes after acquisition; lease expiry, not history retention | `acquired_unix_seconds` | [`codex-router-proxy` runtime maintenance](../../crates/codex-router-proxy/src/server/runtime_config.rs) | `ACTIVE_CLIENT_STALE_AFTER_SECONDS` |
| `state.sqlite` | `active_session_events` | Completed events older than 1 week compact into retained rollups | `event_unix_seconds`; only completed/compactable rows | [`codex-router-proxy` maintenance actor](../../crates/codex-router-proxy/src/server.rs) | `ACTIVE_SESSION_EVENT_RETENTION_SECONDS` |
| `state.sqlite` | `active_session_rollups` | 1 week | `bucket_end_unix_seconds` | [`codex-router-proxy` maintenance actor](../../crates/codex-router-proxy/src/maintenance_actor.rs) | `ACTIVE_SESSION_ROLLUP_RETENTION_SECONDS` |
| `interaction.sqlite` | Former mail and delivery tables (`router_pushes`, `message_send_answers`, `local_cancel_deliveries`, `mail_read_contexts`, `mail_submission_epochs`, `local_delivery_retries`, `pending_poll_mail`, `subscription_delivery_progress`, `board_intent_progress`, `board_intent_scan_cursors`, `mail_retirement_progress`, `installation_peer_directory`, `mail_removal_backfill`) and the `_fleet_sync_*` bookkeeping tables | Dead tables: no code writes or prunes them. Their migrations stay so existing files keep opening; dropping them is a schema change for deliverable D | None | None | None |
| `interaction.sqlite` | Settled `typed_interaction_history` | 2 calendar months from creation; pending approvals and questions remain | `created_at` | [`interaction-storage`](../../crates/interaction-storage/src/interaction_history_store.rs), maintained by the Host | `INTERACTION_RETENTION_MONTHS` |
| `interaction.sqlite` | `approval_routes` | 2 calendar months after last registration or lookup; imported entries start at import time | `last_used_at_ms` | [`interaction-storage`](../../crates/interaction-storage/src/approval_routes.rs), maintained by the Host | `INTERACTION_RETENTION_MONTHS` |
| `provider-operations.sqlite` | Terminal `provider_operations` | 2 calendar months by default; `--provider-operation-retention-days` remains an explicit day override | `terminal_at_ms` | [`collaboration-runtime`](../../crates/collaboration-runtime/src/provider_operation_store.rs), maintained by the Host | `PROVIDER_OPERATION_RETENTION_MONTHS` |
| `provider-operations.sqlite` | `provider_session_records` | Current session inventory; no age pruning | None | [`collaboration-runtime`](../../crates/collaboration-runtime/src/provider_operation_store.rs) | None |
| `session-registry.sqlite` | `lifecycle_records` | 2 calendar months | `retention_at` | [`lifecycle-observation`](../../crates/lifecycle-observation/src/journal_retention.rs), maintained by the Host | `LIFECYCLE_RECORD_RETENTION_MONTHS` |
| `session-registry.sqlite` | `checkpoint_addresses` and corresponding stale `thread_addresses` | 2 calendar months after last observation | `last_seen_ms` | [`lifecycle-observation`](../../crates/lifecycle-observation/src/journal_retention.rs), maintained by the Host | `CHECKPOINT_ADDRESS_RETENTION_MONTHS` |

At startup, the Host imports `approval-routes.json` into `interaction.sqlite` in one transaction and removes it after commit. Invalid route JSON refuses startup and is kept for recovery. The Host deletes `approval-history.json` without importing it. Native approval outcomes are stored as `NativeApproval` records in the existing `typed_interaction_history` table, alongside typed approval and question history, and follow the same two-calendar-month settled-history retention window.

## Space recovery

### Native SQLite files

`state.sqlite`, `session-registry.sqlite`, and `provider-operations.sqlite` use WAL and the shared `sqlite-storage-maintenance` crate. New files set `auto_vacuum=INCREMENTAL` before their first schema write. On existing files, the opener checks the mode and, if conversion is needed, checkpoints WAL, sets `auto_vacuum=INCREMENTAL`, then performs one full `VACUUM` and verifies the resulting mode. It requires free space equal to twice the database-plus-WAL size. This rewrites the database once and can hold its SQLite writer for the duration; the conversion path does not wait on a busy database and defers when it cannot safely proceed. Owners call bounded `PRAGMA incremental_vacuum(64)` steps after retention work, followed by a PASSIVE checkpoint.

SQLite only applies `auto_vacuum` when it is selected before tables are created, or after a full `VACUUM` converts an existing file. See SQLite's [auto-vacuum pragma](https://www.sqlite.org/pragma.html#pragma_auto_vacuum) and [VACUUM documentation](https://www.sqlite.org/lang_vacuum.html).

The owning maintenance paths are [proxy maintenance](../../crates/codex-router-proxy/src/maintenance_actor.rs), [interaction retention](../../crates/collaboration-runtime/src/interaction_retention_worker.rs), [lifecycle journal retention](../../crates/lifecycle-observation/src/journal_retention.rs), and [provider-operation retention](../../crates/codex-router-host/src/collaboration_runtime/provider_retention.rs). The shared conversion and bounded step are in [sqlite-storage-maintenance](../../crates/sqlite-storage-maintenance/src/lib.rs).

### Turso files

`interaction.sqlite` explicitly selects WAL through its opener. Retention deletes pages for reuse, but the current pinned `sqlx-turso` driver does not set Turso's experimental vacuum option and does not expose `VACUUM`; the Router opener does not enable `auto_vacuum` for this store, and no Router Turso store runs VACUUM automatically. See the [pinned driver limitations](https://github.com/agent-collaboration/agent-fleet-sync/blob/b9da4b5f230f81430fdf1140fbb634cf64decd8e/crates/sqlx-turso/README.md#L88-L97).
