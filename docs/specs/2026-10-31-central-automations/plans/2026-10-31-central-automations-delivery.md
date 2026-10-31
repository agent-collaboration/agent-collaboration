# Plan: central automations delivery

```text
plan path: docs/specs/2026-10-31-central-automations/plans/2026-10-31-central-automations-delivery.md
originating planner: plan-implementation
supersedes: none
planning result: ready
governing planning basis:
  kind: reviewed-four-document-design
  Requirements: docs/specs/2026-10-31-central-automations/requirements.md
  Specification: docs/specs/2026-10-31-central-automations/specification.md
  Program Design: docs/specs/2026-10-31-central-automations/program-design.md
  Verification Design: docs/specs/2026-10-31-central-automations/verification-design.md
  review: Advisor four-document-design review + four correction checks, final READY at bfa5020f6 (2026-10-31); U20 F16/F17 closed; owner-answer correction (5-minute precision, interim approver, milestone verification) under Advisor anchor check
  owner approval: plan approved as written on the decisions page, 2026-10-31, with "milestones means it should be verified" and "make sure each milestone is verifiable"; revised in place before any implementation started
  applicability anchors: origin/main 072f4d6c1; fix/router-db-retention d3a831a84+ (PR 1); pinned agent-fleet-sync v0.4.0 b9da4b5; worker 0.8.7
delivery context:
  requested terminal: pr-ready-unmerged
  delivery grouping: single:central-automations
  PR topology: separate-prs — a two-PR stack, decided by the owner ("both 2 prs in the stack can go in together"): PR 1 fix/router-db-retention (retention, vacuum, WAL), then PR 2 design/automations-central (this design, its implementation, migrations, notifications, docs republish). They merge together.
  repositories: agent-collaboration/agent-collaboration (codex-router). agent-fleet-sync is read-only here; the Turso vacuum research reports separately and changes no pin.
```

Planned at branch `design/automations-central`, HEAD `bfa5020f6`, stacked on `origin/fix/router-db-retention`.

## Goal

Automations become central definitions of scheduled messages: one list for the organization, fired by central on the 5-minute grid, and batched per machine. Each machine runs them as today and reports back. The existing per-machine definitions move once, notifications reach each agent at most once every 10 minutes, and `main` carries the owner-decided board design.

**Non-goals:**
- multiple organizations;
- role targets;
- offline creation;
- chasing sent Firings;
- holding Firings on the machine for later release (B);
- per-org offsets;
- the cost measurement (deferred by the owner);
- any change to the agent-fleet-sync pin.

## Current evidence

Code paths are cited in the Program Design, and include:
- the call graph (`call_graph.rs:162–168`);
- `fleet_runtime.rs:54`;
- `AlarmOwner` (`alarm_owner.rs:34–51`);
- `machine_notice_delivery.rs` and `machine_notice_store.rs:14,189–199`;
- `run_admission.rs`, `run_inventory.rs` and `run_lifecycle.rs:21–30`;
- `wakeup_evaluation.rs`;
- `schedule_activation.rs:80`;
- `automation_v1.sql:51,88`;
- `reader_delivery_owner.rs` and `subscription_facts.rs`.

## Milestones and slices

Each slice gets a red build first, then green, with its named seeded wrong builds. Proof rows are the Verification Design's V rows.

| Slice | Milestone | Writes | Depends on | Earns |
| --- | --- | --- | --- | --- |
| **S0** | M0, PR 1: database retention, vacuum, WAL (in progress), and approval state only in `interaction.sqlite` | the storage crates, `sqlite-storage-maintenance`, `docs/architecture/data-retention.md`; `approval_routes` table with a one-time import of `approval-routes.json`, then the file deleted; the `approval-history.json` code and file removed | — | PR 1 gates (below) and the M0 gate |
| **S1** | M1: contracts and limits | `collaboration-protocol/src/limits.rs`; the call-kind enums for `AutomationCall`, the custody envelopes, `FiringReport` and `LocalRunSettled`; `BoardNotice::AutomationBatchReady`; the batch body; schemars annotations; the CLI and MCP schema exposure; the grid-precision validation (`offGrid` with the two nearest grid times; `after` refused for Automations) | — | V5, and V6's entry half |
| **S2** | M2: central catalog | the Fleet face in `collaboration-central-core` (catalog, content revisions, slot and replacement, operation receipts) and the Fleet store migrations on both runtimes; `CoreHost::classify` for the Session caller | S1 | V1 (catalog part), V3, V4 in conformance |
| **S3** | M2: central clock and batches (plus the Tokio central half of the debug verification clock) | `AutomationClock::evaluate` in `on_alarm`; `automation_timing`, `automation_firings` and `automation_batches`; the grid and fold rules; freeze; notice operations; self-healing `wake_at`; the permanent/transient error split | S2 | V2, V6, V8, V10 (central side) in conformance |
| **S4** | M2: central custody | `PullAutomationBatches`, `FetchContents`, `AckAutomationBatches`, `ReportFirings`, `LocalRunSettled` intake; outcome states and `seq` | S3 | V13 (central side) in conformance |
| **S5** | M3: the receiving Router | `collaboration-runtime`: the pull client (connect, hint, `CursorExpired`); local admission (projections with `authority`, receipts, `admit_central_run`, wake rows); the reporter flusher (500 ms or 100); `automation-storage` migrations, including `central_report_outbox`; the debug fault points (later slices add their own points: S6, S11, S7) | S1, S4 | V7 (reporter part), V9, V11, V12, V13 on the rig |
| **S6** | M4: command cutover | `collaboration-service` wake, schedule and instruction operations to the catalog; the new answers; the remote-wake path and the "another service" refusal retired | S2, S5 | V1, V4 on the rig |
| **S7** | M5: the move to central | `collaboration-runtime` handover (freeze, watermark, external occupant, carried waiting, normalization, import, `LocalRunSettled` after acknowledgement); timing workers stop admitting for `central` rows | S5, S6 | V14, V15, V16 |
| **S11** | M3b: sessions on other machines | central `RemoteSessionCreate` and status, plus `machine_work_items`; the target's work-item injection (today's local create and first input); the `collaboration-client` refusal removed; `operation show` and `wait`; the **interim** default approver: each machine's named Chief of Staff session (a per-machine setting), `approverRequired` when none is named, labeled interim with routed approvals as its removal condition | S4, S5 | V18 on the rig |
| **S8** | M6: notification cadence | the reader delivery owner's limiter (`collaboration-service/src/subscription_delivery/reader_delivery_owner.rs`) and `notification_windows` in `interaction.sqlite`; the Host half of the debug verification clock | S1 | V7 |
| **S9** | M7: republish the board design | the decision inventory; the hand-picked docs change on this branch; fixing the two branch contradictions; the public-safety scan of the staged tree and the outgoing ancestry | — | V17 |
| **S10** | M8: end-to-end proof | the `verify-codex-router` journey "central automations across two machines", and the conformance cases | built from day one against stand-ins; run at each gate | runs every V row on the rig; the final full pass |

**Necessary edges:** S1 before S2, S5 and S8; S2 → S3 → S4; S4 and S1 before S5; S2 and S5 before S6; S5 and S6 before S7; S4 and S5 before S11. S9 and S10's building have no dependencies.

**Stand-ins:** S5 may start against a conformance stand-in of the S4 custody calls. Its integration proof needs the real S4. A stand-in proves nothing about the real interaction.

## Milestone verification gates

A milestone counts as done only when its gate below has been observed by the 🛠️ Verifier on the named target and recorded with its evidence. A dependent milestone doesn't start its integration until then. Every gate also needs a seeded wrong build (kept in the working tree, never committed) that fails the same observation. M8 is the final full pass, not the first time anything is checked.

| Milestone | Target | Observed | The wrong build that must fail |
| --- | --- | --- | --- |
| M0 | an isolated signed debug Router on Sunclaw, started over a seeded copy of each store with old rows and an `approval-routes.json` | every store opens in WAL; rows past each window are gone and newer ones remain; free pages are reclaimed; routes are in `approval_routes` and survive a restart; both JSON files are gone; a corrupt routes file is refused and kept | retention off for one store; a route lost in the import; a JSON file left behind |
| M1 | the contracts crate and the debug CLI's and MCP's published schemas (no central yet) | V5 stage one: for each Limit, schema bound = validator = CLI and MCP description; a corrupted stored value is refused; V6's validator property test (10:05 accepted; 10:02, a 7-minute period, cron minute 3 and `after` refused `offGrid` with the two nearest grid times). **Not proven here:** acceptance through real central, CLI and MCP calls, which M2 and M4 own | a schema and validator mismatch; an off-grid time accepted or rounded |
| M2 | conformance on Tokio **and** Cloudflare `workerd`, with a controlled clock, driven through the central contract client (the real CLI and MCP arrive at M4) | V1 catalog, V2, V3, V4, V5 and V6 central stages, V8, V10 and V13 central side. Counted as business effects: one admission per (automation, due time) across a duplicate alarm and a shared notice retry; a 250-entry fixture for one machine splits into 3 Batches of at most 100, each with a stable ID and the same bytes on every resend; each Batch held until acknowledged | a duplicated Firing; a dropped chunk; changed bytes on resend; a Batch over 100 entries; a Batch released before acknowledgement |
| M3 | the rig: two signed debug Routers (A, B) and the local Tokio central, with Haiku sessions | V7 reporter, V9, V11, V12, V13: wakes and schedule runs arrive on B from the central list; kills at each admission point lose and double nothing; reports reach central in order | a duplicate run after a kill; a lost Firing while B is offline |
| M4 | the rig | V1, V4, and V5 and V6 through the real CLI and MCP on A and B: `wake` and `schedule` go to central; off-grid inputs refused with their neighbours; the old cross-machine path is refused | a local definition still created; the old remote-wake path still accepted |
| M3b | the rig | V18: from A, a session created on B with a first input; its route records A's requester as creator; a permission prompt in it reaches B's Chief of Staff and the session resumes on the Chief's decision; `approverRequired` when B has none; offline, retry and refusal behave | a second session or input; a missing default accepted; a route refused after native creation |
| M5 | the rig, with A on the old release first and an owner-shaped fixture | V14, V15, V16: the move with kills at every step; no loss, no double fire; every converted executable timing passes entry validation; the 10:02 one-shot is excluded on the imported definition's own first `due_ms`; an inexpressible cron arrives suspended with its `legacy_timing` and fires nowhere; old-release B keeps its own; history stays readable | a doubled fire at the handover; a definition lost |
| M6 | the rig | V7: a burst of notifications to one session gives one now and one listing 10 minutes later, across a restart; approvals, questions and direct messages stay immediate | a second notification delivery inside the window; a delayed approval |
| M7 | the docs manifest and the public-safety scan | V17 | any scan hit or contradiction |
| M8 | the rig, every V row in one pass at the stack's head | all of the above together | any row |

## Workstreams

All helpers run on Sunclaw, created through the Station Chief and messaged through agent-collaboration.

| Lane | Slices | Helper |
| --- | --- | --- |
| Database (running) | S0 | Worker `01a19238` (Luna xhigh) |
| Contracts → Central | S1 → S2 → S3 → S4 | Sidekick, Sol xhigh |
| Router | S5 → S6 → S11 → S7 | Sidekick, Sol xhigh (starts after S1, with an S4 stand-in) |
| Notifications | S8 | Worker, Sol high (after S1) |
| Docs republish | S9 | Worker, Luna xhigh, plus a separate public-safety review |
| Verifier | S10 | Verifier, Sol high: builds the rig and conformance from day one |
| Advisor | — | `01a18993`, a consult at the S3, S5 and S7 boundaries |

The Lead integrates each lane at its gate. There's one Rust target per worktree, and lanes use separate worktrees (Agent Studio) so builds don't serialize.

## Gates and commands

These run on Sunclaw, never on Sunbook.

- **Per slice:**
  - `cargo check -p <crate>`;
  - the narrow `cargo test -p <crate> <filter>`;
  - `cargo clippy -p <crate> --all-targets -- -D warnings`;
  - `cargo fmt --all -- --check`.
- **Central gate (S2–S4):**
  - `cargo test -p collaboration-central-core` and `cargo test -p collaboration-central-conformance`, on Tokio and on Cloudflare `workerd` (C2).
- **Router gate (S5–S7):**
  - `cargo test -p automation-storage -p collaboration-runtime -p collaboration-service`;
  - `cargo test -p codex-router-host`, with the isolated-test grant;
  - `cargo test -p lifecycle-observation -p interaction-storage`.
- **Repository gates:**
  - the module-cycle ratchet: `cargo nextest run -p workspace-architecture-check --run-ignored only`;
  - the verification tooling: `pnpm --dir verification install --frozen-lockfile`, then `check`, `lint`, `format:check` and `test`;
  - file size limits.
- **Milestone gates:** the table above; the Verifier records each observation with the build, target and evidence path before the milestone counts.
- **Rig:**
  - two signed debug Routers plus a local Tokio central on Sunclaw, driven by the `verify-codex-router` journey;
  - Haiku sessions with a small budget;
  - each V row's passing condition, exactly as written.
- **Merge gate for the stack:**
  - PR 1 and PR 2 CI green;
  - an independent implementation review (Sol xhigh);
  - the version bumped to the next unused number, with `Cargo.lock` updated;
  - both PRs merged together, then the tag, release and Homebrew steps per AGENTS.md.
  - The production Router is never restarted by the agent.

## Obligation-to-proof map

| V | Slice that earns it | Command or observation |
| --- | --- | --- |
| V1 | S2, S6 | conformance catalog plus the rig CLI and MCP on A and B |
| V2 | S3 | conformance with leases expired |
| V3 | S2 | conformance slot and replacement cases |
| V4 | S2, S6 | conformance with the answer dropped after commit; the rig retry |
| V5 | S1 | the limits inventory test (schema against acceptance; decode refusal) |
| V6 | S3 | the grid property test over timing kinds, zones and DST |
| V7 | S8 (+S5 reporter) | the rig notification journey; the controlled-clock flush test |
| V8 | S3 | conformance: empty journal, self-heal, fault injection, splits |
| V9 | S4, S5 | the rig: offline past the notice window, lost replies, crashes |
| V10 | S3, S6 | conformance plus the rig: before and after the cutoff |
| V11 | S5 | the rig kill points |
| V12 | S5 | the rig with Haiku on an empty B |
| V13 | S4, S5 | conformance plus the rig: states, `seq`, sparse flush |
| V14 | S7 | the rig: owner-shaped fixture and kill points |
| V15 | S7 | the rig: an old-release B |
| V16 | S7 | local history reads |
| V17 | S9 | the docs manifest, grep and scan |
| V18 | S11 | the rig: create from A on B, offline, retry, refusal |

## Risks

- **Owed work lost or doubled at the migration, admission or acknowledgement boundaries** (the Advisor's top risk). V9, V11 and V14 must run on the real paths before any "working" claim.
- **Self-healing depends on a later central request**, as specified. A Fleet with no traffic after exhausted alarm retries waits for its next request.
- **Turso migration paths** (the interaction store). Proven on Turso temporary databases.
- **The public repository.** The design-branch history is never pushed; S9 publishes only a selected tree.

## Stop and replan conditions

Stop and return to the owning document (then `plan-update`) if:
- a seam in the Program Design doesn't hold in code;
- a V row can't be observed as written;
- an owner decision changes;
- a migration would rewrite data beyond the design.

A persisted-format change not in the Program Design is a stop for the owner.
