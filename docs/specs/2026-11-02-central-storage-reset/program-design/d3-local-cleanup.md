# Program Design: D3, local cleanup

How D3 realizes U7 (local records), U9 (local copies), U11, U12, U13 on each Router. Evidence: `tmp/d2-inventory/03-local-cleanup.md` (Worker, every local table with final columns, keys, timestamps and JSON columns; every JSON file; every service-id reference). Rules: every table a UUIDv7 primary key (an existing UUIDv7 key stays) and `created_at`/`updated_at`; one prefix per concern; typed columns, JSON only with a written reason; CHECK only for booleans (`AGENTS.md`).

## What you will be able to do

On any Router: every local table name says what it is for, every table has a UUIDv7 key and timestamps, local message copies are removed 60 days after creation, router state lives in SQLite instead of scattered JSON files, and each machine is identified everywhere by its one UUIDv7 machine id.

## Scope

| Store | In D3 | Why |
| --- | --- | --- |
| `interaction.sqlite` (messages, approvals, questions) | yes | U11, U12 |
| `provider-operations.sqlite`, `session-registry.sqlite`, `fleet-cache.sqlite`, `fleet-outbox.sqlite`, `board-cursors.sqlite`, UI preference stores | yes | U11 |
| `automation.sqlite` | no: deleted by D4b | U14 |
| `state.sqlite` (accounts, quota, routing) | **no** | the model proxy is protected (`AGENTS.md` “Servers And HTTP”: work on other surfaces does not change the proxy) |
| project stores | no | project tables are out of scope |
| library tables in these stores | already done in D1 (L7) | |

## Table names (one prefix per concern)

| Today (`interaction.sqlite`) | After |
| --- | --- |
| `router_pushes`, `message_send_answers`, `local_delivery_retries`, `local_cancel_deliveries`, `pending_poll_mail`, `installation_peer_directory`, `mail_read_contexts`, `mail_submission_epochs`, `mail_retirement_progress` | `mail_pushes`, `mail_send_answers`, `mail_delivery_retries`, `mail_cancellations`, `mail_pending_polls`, `mail_peer_directory`, `mail_read_contexts`, `mail_submission_epochs`, `mail_retirement_progress` |
| `board_intent_progress`, `board_intent_scan_cursors`, `subscription_delivery_progress` | `notification_intent_progress`, `notification_intent_scan_cursors`, `notification_subscription_progress` (D4a may delete them when notifications move to wakes) |
| `typed_interaction_history`, `interaction_history_revision`, `approval_routes` | `interaction_history`, `interaction_history_revision`, `interaction_approval_routes` (names only; approval and question behaviour unchanged) |

Other stores keep their concern prefix or gain one (`provider_operations`, `provider_session_records`; `session_journal_metadata`, `session_lifecycle_records`, `session_thread_addresses`, `session_journal_checkpoint`, `session_checkpoint_addresses`; `fleet_cached_records`, `fleet_cache_checkpoint`). Each table's exact typed column list goes on the D3 page for the owner before its queries are rewritten, as in D1.

## JSON files

| File | Decision | Reason |
| --- | --- | --- |
| `ports.json`, `providers.json`, `fleet/enrollment.json` | stay JSON | configuration the owner and CLI edit by hand; a file is the right home |
| `service.json` | stays JSON | the published discovery manifest other processes read; generated, not state |
| `<schema-digest>.json` | stays | immutable generated protocol schema bundle |
| `machine-session/state.json`, `machine-session/machine-notice-progress.json` | **move into SQLite** (`fleet-cache.sqlite`: `fleet_session_state`, `fleet_notice_progress`) | durable protocol state, replaced atomically today by hand; belongs in a store with transactions |
| `snapshot-install-attempts/<project>-e<epoch>.json` | **move into SQLite** (`fleet-cache.sqlite`: `fleet_snapshot_installs`) | durable install correlation |
| `automation-settings.json` | deleted with automations (D4b) | |
| `approval-routes.json` legacy importer | **deleted** (hard cutover; PR 1 already moved routes into `interaction.sqlite`) | |
| `service-identity.json` | **deleted** after the machine-id move | the machine id is the installation peer id (`installation-peer.json`) |

## 60-day removal for local copies

`mail_pushes` and the other `mail_*` tables follow U9: removed 60 days after the message's creation, no matter their state; today's 2-calendar-month pruning (`interaction_retention.rs:35`) is replaced for messages. Approval and question records keep today's retention (D-7).

## The machine id move (owner: “v7 id only for machines”)

Each Router's one id becomes its UUIDv7 installation peer id. At upgrade, the Router rewrites, in one transaction per store, every stored session address and record that carries its old UUIDv4 service id (mail pushes, interaction history, provider operations and session records, session registry, Fleet cache), then deletes `service-identity.json`. The Fleet's `machine_enrollments.service_id` column is dropped once every Router has re-enrolled with the machine id.

**Open, for the owner:** project content also stores session addresses (thread participants, watches, actors), it replicates between machines, and project tables are out of scope. See the D3 page for the choice.

## Not in D3

The proxy's `state.sqlite`; project tables; anything automation (D4a, D4b).
