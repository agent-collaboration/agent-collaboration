# Program Design: D3, local cleanup

How D3 realizes U7 (local records), U9 (local copies), U11, U12, U13 on each Router. Evidence: `tmp/d2-inventory/03-local-cleanup.md` (Worker, every local table with final columns, keys, timestamps and JSON columns; every JSON file; every service-id reference). **Revision r2 (2026-11-06), re-anchored on merged D1** (Router `9ad3c0db0`, library `e3f7937`); evidence `tmp/d3-inventory/2026-11-06-local-stores.md`, which supersedes the earlier inventory. Obligations: [R23–R29](../specification/local-storage.md). Rules: every table a UUIDv7 primary key (an existing UUIDv7 key stays) and `created_at`/`updated_at`; one prefix per concern; typed columns, JSON only with a written reason; CHECK only for booleans (`AGENTS.md`).

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
| library tables in these stores | partly done in D1 (L7): journal, outbox, inbox, report and status tables have UUIDv7 keys and timestamps. Left for D3: the outbox and inbox cancel links (natural keys), the delivery migration table, the hook migration history and the legacy Mailbox tables (below) | R24, R29 |

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
| `ports.json`, `providers.json` | stay JSON | configuration the owner and CLI edit by hand; a file is the right home |
| `fleet/enrollment.json` | **moves into a `fleet_login` table** in the existing Router fleet store (`fleet-cache.sqlite`, a table forget never clears), written by Fleet access, as a hard cutover; no new database (owner: no database sprawl) | one owner for “which central am I enrolled with”; enrollment's forget step never touches it |
| `service.json` | stays JSON | the published discovery manifest other processes read; generated, not state |
| `<schema-digest>.json` | stays | immutable generated protocol schema bundle |
| `machine-session/state.json`, `machine-session/machine-notice-progress.json` | **move into SQLite** (`fleet-cache.sqlite`: `fleet_session_resume_state`, `fleet_session_notice_progress`), which enrollment resets, since they belong to one central's session | durable protocol state, replaced atomically today by hand; belongs in a store with transactions |
| `snapshot-install-attempts/<project>-e<epoch>.json` | **move into SQLite** (`fleet-cache.sqlite`: `fleet_install_attempts`), a table that enrollment's forget step **keeps** and reconciles (an attempt whose project is gone after a reset is retired explicitly), while it clears only the named cache tables | durable install correlation: one attempt per project and epoch, reused (`install_attempts.rs:15–58`); losing it would mint a new identity for an in-flight install |
| `automation-settings.json` | deleted with automations (D4b) | |
| `approval-routes.json` legacy importer | **deleted** (hard cutover; PR 1 already moved routes into `interaction.sqlite`) | |
| `service-identity.json` | **deleted** after the machine-id move | the machine id is the installation peer id (`installation-peer.json`) |

## Library Mailbox tables left by D1

D1's library L5 removes the Mailbox code, but on purpose it leaves the tables that delivery migration 0001 creates in every machine store (`interaction.sqlite` and delivery-node stores) unused and undropped (Lead decision, 2026-11-03, so D1 makes no machine-store schema change). D3 drops them with one appended library migration, listed on the D3 table page for the owner first: `_fleet_sync_mail_items`, `_fleet_sync_mail_controls`, `_fleet_sync_mail_state`, `_fleet_sync_mail_cancel_links`, `_fleet_sync_delivery_receipts`, and `_fleet_sync_inbox_state` if L5's inventory confirms nothing uses it. Router's reserved legacy-name list shrinks with them.

## Migration history in SQLx's shape (R24)

The Router's own migrations and the library's durable-step and delivery migrations already use SQLx's migration table. The receiver's `_fleet_sync_hook_migrations` doesn't: it has only `version TEXT` primary key and `checksum TEXT` (`fleet-sync-receiver/migrations/0002_replicated_schema.sql:3`). One appended library migration rebuilds it in SQLx's shape (`version` integer primary key, `description`, `installed_on`, `success`, `checksum` BLOB, `execution_time`), copying every recorded version with its checksum decoded to bytes. It runs in every store that holds hook history, on every Machine, so it goes on the D3 table page first.

## Non-boolean CHECKs (R25)

`session-registry.sqlite` has two `CHECK(singleton = 1)` constraints (`lifecycle-observation/migrations/202609110001_journal_baseline.sql:1,4`). Those tables are rebuilt anyway to gain UUIDv7 keys and timestamps, and the rebuild drops both checks. The single-row rule moves into the Rust owner, which refuses a second row when decoding.

## 60-day removal for local copies

**What exists (D1).** `InteractionStore::prune_push_records` already removes old local mail in one transaction (`push_records/interaction_retention.rs:34–96`), but with a cutoff of **two calendar months** (`interaction-storage/src/lib.rs:8–9`) and no kind filter, so settled approval and question pushes are removed on the same clock. D3 changes the cutoff, not the mechanism: no second removal pass.

Retention follows **kind**, not prefix. In `mail_pushes` (today's mixed `router_pushes`, `push_line.rs:185`), direct-message, wake, schedule-run and board-activity rows are removed 60 days after creation (U9); approval and question rows keep today's retention (D-7), and their dependent rows live as long as they do. `mail_peer_directory` is a routing lookup, kept while the peer is enrolled. The shared prune transaction gains a per-kind cutoff: message kinds use 60 days from creation, while approval and question rows, and `approval_routes`, keep today's two-calendar-month rule. The guards that keep pending retries, polls and unresolved answers stay as they are.

## The machine id move (owner: “v7 id only for machines”)

Each Router's one active id becomes its UUIDv7 installation peer id (`installation-peer.json`). Two kinds of stored data carry the old UUIDv4 service id, and they are treated differently:

| Kind | Examples | Treatment |
| --- | --- | --- |
| **Live keys** used to route or look things up | `provider_operations.target_service_id`, the `provider_session_records` composite target key, `interaction.sqlite` routing columns (`router_pushes` target service id, `push_records.rs:164–166, 348–355`), session registry addresses, approval routes, Fleet cache keys, pending outbox routing columns | rewritten to the machine id in one explicit cutover |
| **Frozen content and evidence** whose bytes are hashed or replayed | mail envelopes (the library hashes sender and recipient, `delivery_item.rs:119–184`), request canonical hashes, journal inputs and outputs, history | **never rewritten**; the old id stays as historical provenance |

**Cutover, once per Router at startup, before it serves reads, callbacks or publishes its manifest:** write a durable cutover record (old id, new id, the ordered list of stores); rewrite each store's live keys in that store's transaction and mark that store done in the record; resume from the record after a crash; only when every store is done, publish the manifest with the new id and retire `service-identity.json`. Credential references in `fleet/enrollment.json` move through the enrollment owner without touching resolved secrets.

**After cutover:** new records use only the machine id. A session address with the old service id is refused for new routing with “this machine's id is now …”; there is no alias. The old id is kept only as historical provenance: locally in the cutover record, and in the Fleet as `former_service_id` on the machine's enrollment (written at re-enrollment), so historical records can still be labelled with the right machine.

**Sender binding after cutover:** D1 validates a sealed machine sender (which names the old service id) against its typed owner through one fallible binding seam: the Fleet uses `machine_enrollments`; a Router uses itself, then `installation_peer_directory`, then the Fleet lookup (decided 2026-11-03). After this cutover, new senders name the machine id directly, so the seam resolves new mail by identity. Frozen pre-cutover mail keeps resolving through the immutable directory pair, for display and history only.

**Proof:** kill the Router between every pair of stores and before and after retiring `service-identity.json`; it completes on restart with pending approvals and known provider operations intact, and frozen mail still verifies its digest.

**Live work that carries the old id:** messages queued before cutover (their sealed envelopes name the old id, and admission checks the sender and target against the current machine, `mail_submit_admission.rs:51–58`, `mail_push_preparation.rs:85–92`), and subscriptions and watches in project content whose reader is an old-id session (`subscription_windows.rs:88–166`). **Owner decision (2026-11-03, “yes they end”): they end at cutover.** The Router stops new sends, lets its outbox drain, and expires what is left, telling the sender “expired”. Subscriptions and watches whose reader carries the old id lapse, and agents subscribe again. Project records keep session addresses with the old id as historical provenance, labelled through `former_service_id`; old `router://` links and sealed requests naming the old id resolve for display only, not for new routing. Project content is untouched.

## Not in D3

The proxy's `state.sqlite`; project tables; anything automation (D4a, D4b).
