# Verification Design: central automations

This is the plan for proving the Specification's R obligations through V1–V17. Nothing in it has been observed. Revised 2026-10-31 for the review and U12–U18.

## Targets

- **The rig:** two signed, isolated debug Routers (machines A and B) enrolled with a local **Tokio central**, on Sunclaw. Agent sessions use `claude-haiku-5-5`.
- **Both runtimes:** the central behaviour (catalog, clock, batches, idempotent admission, status) also runs in `collaboration-central-conformance` on Cloudflare and Tokio.
- **Fault points:** the rig can drop answers, kill processes at named points, and redeliver items, through the existing conformance fault hooks and the PTY or process harness.
- **Docs:** the published branch.

## "Proven by", for the owner

| Promise | Proven by | Stays unverified |
| --- | --- | --- |
| One list | write on A, read on B, and the reverse | more than two machines |
| No project | every lease expired; it fires on time | — |
| Wake rules | a paused wake holds the slot; replacement rules; each limit | — |
| Safe retries | drop the reply after commit, then retry | — |
| Limits in the contract | each schema bound equals what's accepted; a corrupted row is refused | — |
| 5-minute grid | every timing kind, time zone and DST; never early; collapses | — |
| Notifications once per 10 minutes | 30 notifications give 1 immediate delivery plus 1 listing; DMs, wakes and approvals stay immediate | the real feel in the owner's sessions |
| Batching | 5 due Firings for 2 machines give 2 Batches; a duplicate alarm gives the same bytes; a large set splits | — |
| Cancel at central only | pause before and after the cutoff | — |
| Once, never twice | a crash at each point, then restart | — |
| Runs as today | frozen instruction, continuity, summary after a restart | — |
| Migration | the owner-shaped fixture, normalization, kills at every step | the owner's real data, checked after upgrade |
| Cost | the A slice measured: per-class requests, alarms, incoming WebSocket messages, SQL rows written and active time, for an idle baseline, one Firing, a 5-Firing grid burst, offline catch-up, and retries | anything beyond the measured workload |
| Published design | manifest, grep, public-safety scan | meaning, which is checked by review |

## Coverage rows

| V | R | Observation (target) | Wrong build that fails it |
| --- | --- | --- | --- |
| V1 | R1 | rig CLI and MCP on A and B | a per-machine list |
| V2 | R2 | conformance with leases expired | a lease gate |
| V3 | R3 | conformance plus rig: paused slot, replacement by creator, session and owner, a foreign refusal, limits, replay | "active only" slot checks |
| V4 | R4 | conformance with the answer dropped after commit | a second definition, or a false `centralUnreachable` |
| V5 | R5 | a test that walks the Limits inventory: schema against the CLI, MCP and central acceptance; corrupted stored rows | a schema and validator mismatch; coercion |
| V6 | R6g | a property test over timing kinds, zones and DST, plus rig fixtures | rounding down; two Firings per grid point |
| V7 | R6n | rig: 30 notifications in 3 projects and 3 kinds, plus a DM, wake, approval and question in the same window; overflow; Off and Poll; a remote replication gate | a per-item push; a delayed DM; a lost item |
| V8 | R7f | conformance: a duplicate alarm; a split; same bytes | double admission; changed bytes |
| V9 | R6 | rig: a mixed Batch held while B is offline past the Wake's end; an old-release target | the schedule dropped; the wake late; an old target accepted |
| V10 | R6a | conformance plus rig: pause before and after the cutoff; a lost answer | a false prevention; a chase message |
| V11 | R7 | rig with kill points: after receive; after 2 of 3 entries; after native accept | an entry lost or repeated |
| V12 | R8, R9 | rig with Haiku: wake modes; frozen instruction with A offline; continuity over 2 fresh runs; a summary after B restarts mid-run | a wrong revision; a stuck run |
| V13 | R10 | conformance with reordered and duplicate reports | a regression |
| V14 | R11, R12 | rig: the owner-shaped fixture; kills before and after the cutoff, the commit and the acknowledgement, with admitted, unsubmitted and uncertain work | two owners; an extended end; a double or missing effect |
| V15 | R13 | rig with B on the old release | a double Firing |
| V16 | R14 | local reads before and after | missing history |
| V17 | R15 | docs: manifest, grep, scan over the tree and ancestry | any hit |

## Journeys needed

- `verify-codex-router`: **"central automations across two machines"** (lasting), driving V1, V3, V6, V7, V9–V12 and V14–V16 on the rig.
- **Conformance:** catalog, clock, batch, status and limits cases on both runtimes (lasting): V2–V5, V8, V10 and V13.
- **Cost measurement** (temporary, done once on the first A slice): per-object-class counts from the Durable Object analytics on staging, for the workloads in the table.
- **Docs publication check** (temporary): V17.
