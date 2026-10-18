# Board orchestration design

**Status:** working design, started 2026-10-04 and current as of 2026-10-18. This folder holds the board Lead's design for the board changes that orchestration needs. It is lane P2 of [distributed agent collaboration](../2026-10-04-distributed-agent-collaboration/2026-10-04-distributed-agent-collaboration.md). The owner has assigned this folder's scope: the board, made **fabric-compatible** (R12). The fabric itself is a separate system.

## Files

Read them in this order: requirements, then the specs, then the program designs, then the verification design. Requirements say why and for whom, the specs say what must be observable, the program designs say how it's built, and the verification design says how each obligation is proved.

| File | What it is | State |
|---|---|---|
| [requirements.md](requirements.md) | **why:** U1–U79, gathered from the owner's settled sources | current (2026-10-18) |
| [spec-1-board-and-contracts.md](spec-1-board-and-contracts.md) | **what (shared):** the entities, board obligations, the collaboration API, failures and proof | current |
| [spec-2-project-storage.md](spec-2-project-storage.md) | **what:** one database per project, writer epochs, replication to the project's central copy, leases, takeover and project creation | current |
| [spec-3-central-fleet.md](spec-3-central-fleet.md) | **what:** central copies and their fences, the lease registry, the admin endpoint, machine sessions, metadata, archive and notices | current |
| [program-design-1-board.md](program-design-1-board.md) | **how:** the admission pipeline, views, the collaboration API, upgrade and the trace | current |
| [program-design-2-project-storage.md](program-design-2-project-storage.md) | **how:** project storage on agent-fleet-sync: the board schema, capture and pieces, the machine session client, lease states, seeding and schema identity | current |
| [program-design-3-central-fleet.md](program-design-3-central-fleet.md) | **how:** central's domain design: connection objects, per-project central copies, the lease registry, the admin endpoint and the fleet object | domain design current; its runtime structure is [the central one-core design](../2026-10-16-central-one-core/2026-10-16-central-one-core.md) |
| [program-design-4-messaging.md](program-design-4-messaging.md) | **how:** messages and notifications across machines: the machine outbox, central's ledger and the recipient's inbox | current |
| [verification-design.md](verification-design.md) | **proof:** the targets, observations and wrong builds for the board and central on agent-fleet-sync | current |
| [../2026-10-16-central-one-core/](../2026-10-16-central-one-core/2026-10-16-central-one-core.md) | **what and how for central's build:** one async core injected into every runtime, with the same proof everywhere. Also [what went wrong](../2026-10-16-central-one-core/what-went-wrong.md) with the first build | current |
| [../2026-10-16-agent-fleet-sync/](../2026-10-16-agent-fleet-sync/2026-10-16-agent-fleet-sync.md) | **the library** the board and central build on: replication, leases, sessions and delivery, with its requirements, program design and consumer-fit verification design | current |
| [entity-model.md](entity-model.md) | workspace › board › project › milestone / task / subtask, threads, ADRs, log entries; ground rules | owner-settled 2026-10-02 |
| [activity-record-brief.md](activity-record-brief.md) | the owner brief on keeping a record of what hosted sessions did | reframed as a local-first fact (ADR-1) |
| [open-questions.md](open-questions.md) | owner questions Q1–Q9 | Q2 and Q6 settled; Q1 and Q9 reframed as local-first facts; Q3 is the Lead's call; Q4, Q5 and Q8 parked |

## Fabric compatibility: what the board guarantees

The board must work on one machine today and split across the fabric later without a rewrite. Each guarantee holds even on one machine.

| # | Guarantee |
|---|---|
| 1 | IDs are UUIDv7 minted at the origin and carry their scope (workspace, board, project) |
| 2 | Ordering is per project, not per Router |
| 3 | Project writes carry an admission context: an expected revision, plus an epoch if D1 = B |
| 4 | The change feed is complete, including read marks and acks, with watermarks |
| 5 | Moves are typed requests, accepted or rejected with a reason; there are no silent merges |
| 6 | Cross-project links are immutable IDs; `dependsOn` stays within one project |
| 7 | Server-first and project-scoped records live in separate stores |
| 8 | Each store records its schema version, refuses to write if it's newer, and migrates on handover |
| 9 | The board never delivers. It emits notify intents to interactions, linked to the causing record |
| 10 | Every actor records its machine and session, ready for caller-bound identity |
| 11 | Roles are selectors with an assignment revision and resolution provenance; the fabric resolves a role to a live session |
| 12 | Lifecycle facts are typed: close cutoff, design complete, dropped with a reason |
