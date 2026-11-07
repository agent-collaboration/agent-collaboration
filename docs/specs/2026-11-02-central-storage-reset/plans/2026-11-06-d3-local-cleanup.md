# Plan: D3, local cleanup

## Canonical record

```text
plan path: docs/specs/2026-11-02-central-storage-reset/plans/2026-11-06-d3-local-cleanup.md
originating planner: plan-implementation
supersedes: none
planning result: ready (code starts only after the owner approves D3's table set, tmp/2026-11-05-after-d1-decisions.html section 4, and after D1's production switch)
governing planning basis:
  kind: reviewed-four-document-design
  Requirements: requirements/mail.md (U7, U9 local), requirements/local-storage.md (U11–U13)
  Specification: specification/local-storage.md (R23–R29), entities.md (E13, E14)
  Program Design: program-design/d3-local-cleanup.md (r5 plus the table naming at 3001e8bbd)
  Verification Design: verification-design/d3-local-cleanup.md (Q1–Q7)
  review: codex-router.review-d1-pr215/tmp/review/2026-11-06-d3-design-review.md (5) → 2026-11-06-d3-design-rereview.md (F1–F5 closed; F6 → closed; F7 → closed at 0eeb9e853; 0 open)
  table page: tmp/2026-11-06-d3-table-page.html; column lists: tmp/d3-inventory/2026-11-06-d3-column-lists.md
  applicability anchors: codex-router origin/main cfe99ac02 (plus #224 when merged); agent-fleet-sync origin/main e3f7937
delivery context:
  requested terminal: pr-ready-unmerged
  delivery grouping: single:d3-three-lanes
  PR topology: one-pr per repository (library first; the Router pins it by rev)
  repositories: agent-collaboration/agent-fleet-sync, agent-collaboration/agent-collaboration (codex-router)
planned at: design/central-storage-reset @ 3001e8bbd
```

## Goal

Each Router's storage follows the owner's rules: concern-prefixed names, UUIDv7 keys with timestamps, CHECKs only on booleans, JSON only with a stated reason, local message copies removed 60 days after the Message's creation, and one UUIDv7 machine id everywhere new. Every row survives the upgrade.

**Not in scope:** the model proxy's state store, project stores and their content, automation tables (D4a/D4b).

## Lanes and order

```text
Library lane:  L1 hook history + cancel-link keys + Mailbox drop ─────────────┐
Router lane A: A1 interaction.sqlite ─► A2 retention ─► A3 approval ids ──────┤
Router lane B: B1 other stores + JSON moves ──────────────────────────────────┼─► G1 Tokio ─► G2 staging
Central+switch: C1 Fleet switch call ─► C2 Router switch + subscription lapse ┘
```

| Edge | Why |
|---|---|
| L1 → A1, B1, C1 | Router migrations run the library's migrations in the same stores, and the Router pins the library by rev |
| A1 → A2 | the retention change rewrites queries over the renamed `mail_*` tables |
| A1, B1 → C2 | the switch rewrites live keys in the renamed tables and records progress in `machine_identity_switch*` |
| C1 → C2 | the Router's switch calls the Fleet's switch first |
| all → G1 → G2 | Q1–Q7 run on a populated upgrade, after every slice |

Lanes A, B and C run in parallel in separate worktrees once L1 lands.

## Slices

### L1 (library): hook history, cancel-link keys, Mailbox drop
- **Reads:** PD "Migration history", "Library Mailbox tables"; table page; library `fleet-sync-receiver` `hook_migrations.rs`, `schema_reads.rs`, `0002_replicated_schema.sql`; `fleet-sync-delivery/migrations/0001`, `0008`.
- **Change:** one appended receiver migration rebuilds `_fleet_sync_hook_migrations` with SQLx columns plus a UUIDv7 id and timestamps, copying every version with its checksum decoded to bytes. Initializer, reads and writer change together, keeping the applied-version and checksum checks, rollback, and the local-table-only guard. One appended delivery migration gives the cancel-link tables UUIDv7 ids (both item ids stay unique) and drops the five unused Mailbox tables, plus `_fleet_sync_inbox_state` only if a reference search proves it unused.
- **Wrong behavior:** a reopened populated history rejected as corrupt; a new hook migration that can't be written; a changed checksum accepted.
- **Proof:** Q2's hook cases in the library: upgrade a populated history, initialize twice, apply a new migration, refuse a changed or malformed checksum with rollback; schema readback of the dropped tables.

### A1 (Router): `interaction.sqlite` renames and rekeys
- **Reads:** table page "renames" rows for messages; column lists `interaction.sqlite`; `crates/interaction-storage/migrations/`, its query modules.
- **Change:** one appended migration renames tables per the table page, adds UUIDv7 ids where the key isn't UUIDv7 (old keys stay unique), adds timestamps (text timestamps converted), recreates indexes under the new names, and creates `machine_identity_switch` and `machine_identity_switch_stores` (empty). Queries are rewritten to the new names; sqlx checked queries regenerated.
- **Wrong behavior:** a lost row; a coerced bad value; an approval or question journey that changes.
- **Proof:** Q1 and Q2 on this store, from a populated pre-D3 fixture copy; Q3's approval, question and message-send journeys.

### A2 (Router): kind-aware 60-day deadline
- **Reads:** PD "60-day removal"; `push_records/interaction_retention.rs`, `mail_push_projection.rs`, `message_send_answers.rs`.
- **Change:** the existing prune transaction gives message copies a deadline of the Message's creation time (from its UUIDv7 id) plus 60 days. In the same transaction it ends dependents past the deadline: pending retries and polls are removed, and unresolved send answers become "expired". Approval and question rows and `interaction_approval_routes` keep today's two-calendar-month rule.
- **Wrong behavior:** a copy kept on its settlement clock; a pending dependency keeping a copy alive; approvals removed at 60 days.
- **Proof:** Q4 with the debug mail clock: a late receipt, settlement near day 60, pending retry/poll/answer, and `message show` after the deadline.

### A3 (Router): Router-generated ids
- **Change:** the native approval broker's request id uses UUIDv7 (`native_approval.rs:128`); the UUIDv4 helper is no longer used for new ids.
- **Proof:** Q2's case: a new approval after upgrade has a UUIDv7 request id; old approval records still resolve.

### B1 (Router): other stores and JSON moves
- **Reads:** table page rows for provider sessions, session registry, Fleet cache, Fleet outbox, cursors, UI; column lists; `fleet_enrollment_file.rs`, `machine_session_store.rs`, `machine_notice_progress_store.rs`, `install_attempts.rs`, `service_identity_storage.rs`, `startup_composition.rs:278` (importer).
- **Change:** per-store appended migrations for renames, ids, timestamps; session registry `CHECK(singleton = 1)` removed with the one-row rule in Rust. New Fleet cache tables `fleet_login`, `fleet_session_resume_state`, `fleet_session_notice_progress` and `fleet_install_attempts`; their owners read and write SQLite, and on first start import the old JSON file once, in the same owner, then delete it. The legacy `approval-routes.json` importer is removed.
- **Wrong behavior:** a moved file still read later; a lost enrollment or resume cursor; a new install-attempt identity minted for an in-flight install.
- **Proof:** Q1 and Q2 on these stores; Q5's file moves and restart readback.

### C1 (central): Fleet "switch machine id" call
- **Reads:** PD "The Fleet side"; `machine_enrollment.rs:278`, `mail_submit_admission.rs:66`; the D3 table page central row.
- **Change:** `machine_enrollments.former_service_id` (appended migration, nullable). A Fleet call from the machine's own authenticated session, in one transaction: set the active id to the peer id, record the former id, refuse if another enrollment already uses that id, and replay idempotently. Both backends (Tokio, Cloudflare).
- **Wrong behavior:** a second machine row; a switch by another machine's session; a replay that changes anything.
- **Proof:** central tests per rule plus conformance on both backends; Q6's Fleet half.

### C2 (Router): the switch and the subscription lapse
- **Reads:** PD "Cutover" and "Live work that carries the old id"; `subscription_window_restore.rs:119`, `subscription_window_scan.rs:48`, `follow_window_proposals.rs:112`; library `client_pump.rs:44, 60`.
- **Change:** at startup, before serving: write the switch record; if enrolled, call the Fleet switch with the dispatch state kept (`not sent`, `sent, outcome unknown`, `done`). An unenrolled Router records "no central sign-in". Only `not sent` falls back to the old id. Each store's live keys are rewritten in its own transaction, then the manifest is published with the new id and `service-identity.json` retired. Old-id sends are refused with the new id, and queued old-id messages end as expired. The subscription delivery owner skips readers whose address names a former machine id (from the Fleet's record) at restore, scan and proposal.
- **Wrong behavior:** a half-switched Router serving; an old-id fallback after a possibly committed switch; a restored old-id subscription.
- **Proof:** Q6 (crash at each step, lost reply, unreachable Fleet before send, never-enrolled root) and Q7.

### G1 / G2
G1: the Verifier runs Q1–Q7 on Tokio against populated fixture roots (a copied debug root that ran D1's journeys, approvals, questions and provider operations). G2: Q2's central hook case and Q6's Fleet switch on staging Cloudflare.

## Gates per PR

Per package, never multi-package full runs on shared Macs: nextest for each changed package through the signing runner, clippy `-D warnings`, fmt, the module-cycle ratchet, the verification package, both central backends for C1. Next unused workspace version at merge. One independent Sol xhigh review per PR.

## Risks and stop conditions

- **Migration on real data:** every migration is tested on a populated copy, never only empty stores (the D1 rule). Stop if any row can't be converted without inventing a value.
- **LuLu:** Router and central integration runs on this Mac need the debug identifiers already allowed. A new test binary that needs the network may hit the firewall; check the LuLu rule before debugging transport (memory note).
- **Stop** if a table, column or file beyond the approved table page is needed, or if the switch needs a central API change beyond the one call.
