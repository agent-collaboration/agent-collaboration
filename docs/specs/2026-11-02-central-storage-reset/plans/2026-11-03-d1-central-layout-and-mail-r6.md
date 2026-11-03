# Plan: D1, central layout and mail (with D1b)

## Canonical record

```text
plan path: docs/specs/2026-11-02-central-storage-reset/plans/2026-11-03-d1-central-layout-and-mail-r6.md
originating planner: plan-update
supersedes: docs/specs/2026-11-02-central-storage-reset/plans/2026-11-03-d1-central-layout-and-mail-r5.md
planning result: ready (review closed F1–F5; F6 decided by the owner: journal tables get timestamps)
governing planning basis:
  kind: reviewed-four-document-design
  Requirements: docs/specs/2026-11-02-central-storage-reset/requirements/index.md
  Specification: docs/specs/2026-11-02-central-storage-reset/specification/index.md
  Program Design: docs/specs/2026-11-02-central-storage-reset/program-design/d1-central-layout-and-mail.md
  Verification Design: docs/specs/2026-11-02-central-storage-reset/verification-design/d1-central-layout-and-mail.md
  review: tmp/d1-review/review-e6d85e9b3.md (F1–F4) → verify-6fb9b616a.md (F1–F3 closed; F5, F6 new) → verify-de0c0badb.md (F5 closed) → verify-a6c88b2a4.md (F4 closed; F6 awaiting owner)
  applicability anchors: codex-router origin/main fb3cf6b5 (0.1.121); agent-fleet-sync origin/main b9da4b5 (v0.4.0)
delivery context:
  requested terminal: pr-ready-unmerged
  delivery grouping: single:d1-with-d1b
  PR topology: one-pr per repository (owner: "d1 and d1b can be converted together. one pr")
  repositories: agent-collaboration/agent-fleet-sync, agent-collaboration/agent-collaboration (codex-router)
planned at: design/central-storage-reset @ 6fb9b616a
```


## Changed from 2026-11-03-d1-central-layout-and-mail-r5.md

| Change | Why | Invalidates |
| --- | --- | --- |
| New library slice **L7 journal timestamps**: `durable-steps` adds `created_at` and `updated_at` to `_fleet_sync_operations`, `_fleet_sync_steps`, `_fleet_sync_schema`, `_fleet_sync_migrations`, with an additive migration that runs in every store (Fleet, project objects, machines). Edge L1 → L7; L7 before the library tag | owner decided F6 = B (“yes they should get timestamp columns”) | the journal-table fence in the earlier packets; VD O8 now expects timestamps on those tables too |

## Earlier change: from 2026-11-03-d1-central-layout-and-mail-r4.md

| Change | Why | Invalidates |
| --- | --- | --- |
| L2 splits: **L2a** per-submitter status scoping (status state one row per submitting machine, cursor scoped per submitter) lands first; **L2b** push, receipt, report and content release follows. Edge **L2a → C1–C3 integration gate** | merging Connections into the Fleet leaves one status-stream owner per store (`status_stream.rs:99–128`, global cursor `0001_delivery_custody.sql:21–22`; central appends Mailbox publications through it, `mail_status_receiver.rs:76–83`, `session_scope.rs:161, 186`), so the second submitting machine fails even with Mailbox objects still present. Found by the Central Implementer | the C1–C3 integration target before L2a lands |

## Earlier change: from 2026-11-03-d1-central-layout-and-mail-r3.md

| Change | Why | Invalidates |
| --- | --- | --- |
| New **proof controls** (below) are part of the slices that own each boundary, built only under the `conformance` feature or in debug builds | the Central Verifier's challenge (`tmp/d1-verify/2026-11-02-feasibility.md`) returned `verification-design-gap`: O3, O4, O5, O7, O8 had no control that could drive them; VD corrected at c133916d8 | running O3, O4, O5, O7, O8 before their control exists |
| O5 and O6 are staging-only; C7 drives the rest on Tokio and declares O5/O6 | no local equivalent for a staging wipe or Cloudflare CPU budget | “every journey on Tokio” in C7 |

### Proof controls

| Control | Built in | Used by |
| --- | --- | --- |
| named crash points at Fleet admission, Push result, Report, Status publication | C4 | O3 |
| named crash point at B inbox acceptance; Router injected clock (debug/conformance only); per-holder row readback | C5 | O3, O4 |
| barrier inside forget-on-enroll | C5 | O5 |
| persistent project-object fault until cleared | C3 | O7 |
| conformance endpoint returning the Fleet object schema on Cloudflare | C1 (with C6 for the final table set) | O8 |

## Earlier change: from 2026-11-03-d1-central-layout-and-mail-r2.md

| Change | Why | Invalidates |
| --- | --- | --- |
| New library slice **L6 lease-change hook**: a consumer callback inside every `fleet-sync-lease` lease-change transaction. Edges L1 → L6 → C2 | C2 must commit a lease change, its control notices and the directory update together, and the lease library owns those transactions privately (`change_completion.rs:168, 290`). Found by the Central Implementer; the Lead chose an explicit hook over a central wrapper around private saves | starting the atomic lease and notice part of C2 before L6 lands |

## Earlier change: from 2026-11-02-d1-central-layout-and-mail.md

| Change | Why | Invalidates |
| --- | --- | --- |
| C1, C2 and C3 are one batch with one integration gate at its end, not three separately proven slices | C1 alone cannot attach every machine to one Fleet object while today's per-machine notice store rejects a second machine (`machine_notice_store.rs:86–107`, `session_migrations/0001_machine_notices.sql`); the per-machine rows that fix it are C2's. Found by the Central Implementer before editing | the per-slice proof points after C1 and after C2 |
| The Mailbox address and the Registry/Mailbox mail chain stay inside the branch until C4 removes them | mail still requires `ObjectAddress::Mailbox` (`registry_mail_calls.rs:69–75`, `mail_submit_admission.rs:120`; separate engine `mailbox_runtime.rs:216–255`); its replacement needs L2 | the reading that C1 deletes every address but Fleet and Project. This is an intermediate state of the branch only; the PR ships without it |

## Goal and scope

Deliver U1–U4, U6 (messages), U7 (mail records), U9 (inbox and outbox) and the Fleet-store part of U11, as the Program Design specifies: one Fleet object plus project objects on Tokio and Cloudflare; machine outbox → Fleet outbox → machine inbox with push; every Fleet store table on the full storage rule (D1b).

**Not in this plan:** Fleet-originated messages and remote create (D2); Router app table renames, JSON files, moving session addresses to the machine id (D3); wakes and notifications (D4a); schedules (D4b); project tables (out of scope).

## Rules every slice follows

From `AGENTS.md` and owner rules: UUIDv7 ids only; SQL CHECK only for booleans; enum and range rules in Rust; files at most 1,000 lines; unit tests colocated as `*_test.rs`, integration and end-to-end only under `tests/integration/*` and `tests/e2e/*`; no wall-clock tests (causal signals or injected clocks); debug Router only, Luna test sessions, nothing against production or the owner's main accounts; one debug `target/` per worktree, nothing built into `/tmp`; `cargo run`/`cargo test` through `scripts/cargo_debug_signing_runner.sh`; `git commit --no-gpg-sign` in helper sessions; no `git stash`; CI stays under 30 minutes; module-cycle ratchet (`workspace-architecture-check`) never grows.

## Orchestration across the two repositories

1. **agent-fleet-sync PR** on branch `feat/fleet-relay-outbox`: L1–L4 come first. During development codex-router depends on that branch by git revision. **L5 (removing the Mailbox role) waits until codex-router C4 and C5 no longer use it**; only then is the library finished.
2. After L5, the library PR merges and is tagged **v0.5.0**, before the codex-router PR merges; codex-router's `Cargo.toml` then pins `tag = "v0.5.0"` and `Cargo.lock` is refreshed.
3. **codex-router PR** (slices C1–C7) on `feat/central-storage-reset-d1`, created from `design/central-storage-reset` so the design set travels in the same PR.
4. End-to-end proofs (O1–O11) pass only with both sides; the Lead owns that gate.

## Slices

### Library (agent-fleet-sync)

| Slice | Kind | Obligations | Reads | Write surfaces | Focused proof (wrong behavior it must fail on; oracle) |
| --- | --- | --- | --- | --- | --- |
| **L1** typed delivery records | contract | R9, R10, R13 | PD “Delivery library tables”, “Machine store changes”; `outbox_record.rs`, `inbox_record.rs`, `0001_delivery_custody.sql` | `fleet-sync-delivery` migrations, record types, query SQL and metadata | a record that loses `valid_until`, cancel state, settlement bytes or `last_report_seq` after content release fails; a stored UUIDv4 id is refused (PD outbox column list) |
| **L2** push, receipt, report | vertical | R7, R9, R10, R11 | PD “Mail protocol”, “Who may answer”, failure table | `fleet-sync-protocol` (new Push server call and answers; Report answers), inbox pushed acceptance, outbox content release after the next holder's commit, per-submitter status | duplicate push → `replayed` with current status; changed digest → `conflict`, original untouched; report from a non-recipient → refused; content kept until receipt (PD Mail protocol answers) |
| **L3** 60-day removal | vertical | R14 | PD “Machine store changes”, Spec R14 | removal task and too-old refusal | with an injected clock a row is present at day 59 and gone at day 60; a 61-day-old id is refused (Spec R14) |
| **L4** typed lease and session tables (D1b) | contract | R6, R13 | PD “Moved and converted to typed columns” | `fleet-sync-lease`, `fleet-sync-session` migrations, records, queries | lease and session behaviour tests keep passing against typed columns; no `record` JSON column remains except with a stated reason (PD rule) |
| **L5** remove the Mailbox role | cleanup | R1 (no per-machine custody) | PD “Deleted from central” | `mail_storage`, `mailbox_*`, fetch cycle, `fleet-sync-service` per-peer delivery host | removed with a dead-contract proof: the codex-router branch builds and passes with no Mailbox use |
| **L6** lease-change hook | contract | R2, R5 (atomic lease change and notices) | PD “Lease-change hook” | `fleet-sync-lease` change completion, registration and retirement tasks | the hook runs inside the same transaction: a failing hook rolls back the lease change; a successful change and the hook writes commit together (PD hook paragraph) |

Edges: L1 → L2a → L2b → L3 (L2a = per-submitter status scoping; L2b = push, receipt, report, content release); **L2a → C1–C3 integration gate**; L1 → L4 (shared migration harness); L2 → L5; **C4 and C5 → L5** (the Router and central no longer use the Mailbox); L1 → L6 → C2 (the atomic lease and notice work in C2). L4 carries the same owner gate as C6: each lease and session table's typed column list goes to the Lead for `d1.html` before its queries are rewritten.

### codex-router

| Slice | Kind | Obligations | Reads | Write surfaces | Focused proof |
| --- | --- | --- | --- | --- | --- |
| **C1** one Fleet object (batch C1–C3) | contract + integration | R1, R2, R4 | PD “Components”, “Stores after D1”, “One journal, one dispatcher, one alarm” | `core_contract` (addresses Fleet and Project only), `call_graph`, `object_bootstrap`, `object_runtime`, Tokio `object_keys`/`native_objects`/socket serving, Cloudflare `object_location`/`object_shell`/`platform`/`cloudflare.config.ts` | a socket attaching anywhere but the Fleet fails; a task kind not registered with the one engine fails the mixed-kind restart test (PD dispatcher section) |
| **C2** Registry into Fleet | vertical | R2, R5 (central side), R15 | PD Fleet tables (`machine_enrollments`, control notices, forwards), “Deleted from central”, “Lease numbers and the staging wipe” | enrollment, lease authority, `lease_incarnation`, control notices admitted in the lease transaction, two-sided forwards | unenrolled submit refused with nothing stored; lease change and its control notices commit together; two origins forwarding to one holder keep separate records (PD tables) |
| **C3** Fleet ↔ project boundary | vertical | R3 | PD “Fleet and project objects” | `copy_calls`, `copy_write_calls`, `copy_directory_summary`, call graph | a project call from any caller but the Fleet is refused; a failing project object does not block mail or another project (PD containment rule) |
| **C4** Fleet mail | vertical | R7, R9–R12, R15 | PD “Mail protocol”, failure table | mail admission into Fleet outbox, Push dispatch, push result, report handling, status to submitter; delete Mailbox/Registry mail chain (`mail_submit_calls`, `registry_mail_calls`, `mailbox_*`, `mail_status_receiver` routing) | each failure-table case at the Fleet boundary, with injected clocks and causal waits |
| **C5** Router mail runtime | vertical | R7, R8, R9, R11, R12, R14, R5 (Router side) | PD “Machine store changes”, “Mail protocol”, “Lease numbers and the staging wipe” | `machine_session.rs` (Push handling; delete MailWaiting fetch and the 5-minute receive poll), `mail_delivery_runtime*`, enrollment forget-on-enroll, 60-day removal | a pushed item is committed before the receipt; same-machine send writes the inbox only; forget-on-enroll resumes after a crash (PD) |
| **C6** typed Fleet directory tables (D1b) | contract | R6, R13 | PD “Moved and converted to typed columns” | `core_migrations`, `sql/fleet/*`, generated wrappers | **gate:** column lists for each table posted on `d1.html` for the owner before that table's queries are rewritten; Fleet directory tests pass on typed columns |
| **C7** verification journeys | proof-only | VD O1–O11 | VD | `.agents/skills/verify-codex-router` journey extensions (`needed:` items), `verification/` cases | each journey driven once on Tokio by the Verifier before the full run |

Edges: **C1, C2, C3 are one batch** (C1 composes the Fleet object, C2 moves Registry and Connection duties into it with per-machine notice and forward rows, C3 the project boundary; the Mailbox address and its mail chain stay in place until C4); C3 → C4 (needs L2); C4 → C5 (needs L2, L3); C1 → C6 (needs L4 for lease/session); C2, C3, C4, C5, C6 → C7. Library-to-router: L2 → C4, C5; L3 → C5; L4 → C6. Router-to-library: C4, C5 → L5.

**Grouping choice:** one vertical grouping per repository; slices are ordered so each leaves its repository building against the library revision it pins at that point. Alternative considered: a separate PR for D1b. Rejected by the owner's one-PR decision.

## Verification Design rows → slice and command

| VD row | Earned in | Run by | Command or journey |
| --- | --- | --- | --- |
| U1 (O2, O7) | C3, C7 | Verifier | Central runtime + Central staging journeys with three Routers and two projects |
| U2 (O2) | C1, C7 | Verifier | same, store listing |
| U3 (O5) | C5, C7 | Verifier | Central staging wipe and re-enroll case |
| U4 (O1, O3, O9, O11) | C4, C5, C7 | Verifier | Cross-machine journey with a queue-mode message held on a not-running recipient session, and the failure matrix |
| U6 (O10, O6) | C5, C7 | Verifier | online timing on both backends; staging load |
| U7 (row readback) | L1, C5, C7 | Verifier | row readback after O1 and O9 |
| U9 (O4) | L3, C5, C7 | Verifier | injected-clock retention case |
| U11 (O8) | C6, C7 | Verifier | Fleet schema listing |

## Lanes and batches

| Lane | Slices | Implementer | Paired Verifier | Worktree |
| --- | --- | --- | --- | --- |
| Library | L1–L5 | 🐒 Sol 6.1, high | 🔬 Sol 6.1, high | agent-fleet-sync `feat/fleet-relay-outbox` |
| Central | C1, C2, C3, C4, C6 | 🐒 Sol 6.1, high | 🔬 Sol 6.1, high | codex-router `feat/central-storage-reset-d1` |
| Router | C5 | 🐒 Sol 6.1, high | shares the Central Verifier, observing O1, O3, O5, O9 | fork of the Central worktree, merged back per slice |
| Journeys | C7 | the Verifiers | n/a | Central worktree |

Batch boundaries: after L2 (library Verifier checks push and receipt semantics against the failure table before C4 starts); after C4+C5 (first two-Router Tokio run of O1 and O3 before C6 and the staging runs).

## Quality gates

`cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo nextest run` for each changed package, then the workspace once at the end; `cargo nextest run -p workspace-architecture-check --run-ignored only`; `pnpm --dir verification check|lint|format:check|test`; Demistify check for changed pages. Before merge: workspace version bumped to the next unused semver with `Cargo.lock`; CI green under 30 minutes.

## Risks and stop conditions

- **One Fleet object's load on Cloudflare** (O6) shows the request or CPU budget won't fit → stop, bring evidence to the owner (owner said “unless durable objects were gonna break”).
- **A failure-table case cannot be met** with the Mail protocol as specified → program-design gap, back to the Lead.
- **D1b column lists** not yet posted for the owner when a table's query rewrite is ready → that table waits.
- **Disk:** each worktree's `target/` grows tens of GB; check `df` before builds and `cargo clean --workspace --profile dev` per AGENTS.md when large.
- **False green:** a test that passes through a mocked Fleet or a fake inbox does not count for any VD row.
