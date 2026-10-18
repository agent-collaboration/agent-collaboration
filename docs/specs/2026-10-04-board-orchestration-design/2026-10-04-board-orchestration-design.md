# Board orchestration design

> This navigation index preserves the earlier design chronology. The current requirements, specifications and program designs govern; historical guides are labelled explicitly. Published files exclude private working reports and the two superseded draft reports.

**Status:** working design, 2026-10-04. This folder holds the board Lead's design for the board changes that orchestration needs. It is lane P2 of distributed agent collaboration (unpublished source). The owner has assigned this folder's scope: the board, made **fabric-compatible** (R12). The fabric itself is a separate system.

## Files

| File | What it is | State |
|---|---|---|
| [requirements.md](requirements.md) | **why:** U1–U39, gathered from the owner's settled sources; U32–U39 are the replication and lease decisions of 2026-10-08/09 | current (2026-10-09) |
| [spec-1-board-and-contracts.md](spec-1-board-and-contracts.md) | **what (shared):** entities E1–E22, E28–E29 (workstreams, the work trail), board obligations, the collaboration API, failures, proof | rev 4 (2026-10-11): workstreams, lightweight tasks, workspace ADRs, the project decision register and the work trail (U40–U43); failure kinds for replication, leases and sessions; the API part shipped in 0.1.81 |
| [spec-2-project-storage.md](spec-2-project-storage.md) | **what:** one Turso database per project, writer epochs, CDC replication to the project's central copy, lease expiry, takeover | rev 2 (2026-10-09): replication cutover after the Turso Sync break; in review |
| [spec-3-central-fleet.md](spec-3-central-fleet.md) | **what:** the central fleet: central copies and their fences, the lease registry, the admin endpoint, machine sessions, metadata, archive, notices | rev 2 (2026-10-09); in review |
| [program-design-1-board.md](program-design-1-board.md) | **how:** admission pipeline, views, the collaboration API (one stateless MCP server), upgrade, trace | rev 2 (2026-10-07); open owner decision O1 (operator socket) |
| [program-design-2-project-storage.md](program-design-2-project-storage.md) | **how:** project stores, CDC capture and pieces, the machine session client, lease states, seeding, schema identity | rev 2 (2026-10-09); in review |
| [program-design-3-central-fleet.md](program-design-3-central-fleet.md) | **how:** central on Cloudflare in Rust: connection objects, per-project central copies, the lease registry, the admin endpoint, the fleet object | rev 2 (2026-10-09); in review |
| [entity-model.md](entity-model.md) | workspace › board › project › milestone / task / subtask, threads, ADRs, log entries; ground rules | owner-settled 2026-10-02; one addition proposed (`kind: spike \| build`) |
| board-state-spec.md (unpublished source) | the 2026-09-30 → 10-02 working spec: records, rules, views, pilot results | **superseded as the spec** by requirements.md + specs 1–3; kept as source evidence (problem rows, pilot findings) |
| [activity-record-brief.md](activity-record-brief.md) | the owner brief on keeping a record of what hosted sessions did | reframed: a local-first fact (ADR-1), so the storage question moves to P5 |
| what-we-learned.md (unpublished source) | plain-language report from 2026-10-01 | historical |
| [open-questions.md](open-questions.md) | parked owner questions Q1–Q9 | Q2 settled (Router owns shapes); Q6 settled (several replay cases); Q1 and Q9 reframed as local-first facts; Q3 is the Lead's call; Q4, Q5 and Q8 still parked |

## Fabric compatibility: what the board guarantees

The board must work on one machine today and split across the fabric later without a rewrite. Each guarantee holds even on one machine.

| # | Guarantee | Today (main `7a8cbd89`) | Milestone |
|---|---|---|---|
| 1 | IDs are UUIDv7 minted at the origin and carry their scope (workspace, board, project) | message IDs are caller-minted UUIDv7; scope is implicit | M2.2 |
| 2 | Ordering is per project, not per Router | one counter per Router (`storage_support.rs:255`) | M2.2 |
| 3 | Project writes carry an admission context: an expected revision, plus an epoch if D1 = B | none | M2.2 / M2.2b |
| 4 | The change feed is complete, including read marks and acks, with watermarks | acks produce no activity (`inbox_records.rs:216–257`) | M2.2 |
| 5 | Moves are typed requests, accepted or rejected with a reason; there are no silent merges | designed (`TransitionRejected`) | M2.1 |
| 6 | Cross-project links are immutable IDs; `dependsOn` stays within one project | not modelled | M2.1 |
| 7 | Server-first and project-scoped records live in separate stores | one `project-board.sqlite` | M2.4 (D4) |
| 8 | Each store records its schema version, refuses to write if it's newer, and migrates on handover | one migration history | M-X1 |
| 9 | The board never delivers. It emits notify intents to interactions, linked to the causing record | the board pushes through the automation store | M2.7 |
| 10 | Every actor records its machine and session, ready for caller-bound identity | `SessionRef` carries `serviceId` | M2.5 / P1 |
| 11 | Roles are selectors with an assignment revision and resolution provenance; the fabric resolves a role to a live session | seats on threads | M2.5 |
| 12 | Lifecycle facts are typed: close cutoff, design complete, dropped with a reason | partly | M2.6 |

## Spec revision plan (M2.1)

- **Keep:** the entity model, the records, the move rules, the views, the interaction protocol.
- **Add:** the guarantees above, the placement rule (decisions server-first, facts local-first), and role selectors.
- **Move out to the orchestration and Inspector lane:** guard registry content, oracle pins, pilot scoring and results. Keep only the record shapes Router validates.
- **Re-anchor:** code citations moved in #121 and #122. Re-check every `file:line`.
- **Rename:** "unit" becomes "task"; the Brief keys on `taskId`.
- **Validation:** the replay cases (M2.0, starting with #112 Messaging) test this revision.
