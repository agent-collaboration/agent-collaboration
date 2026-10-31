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
| Cost | not measured now (owner, 2026-10-31: "we don't need to do that now, let's get the design right"); the cost page stays a planning model | the real per-firing cost |
| Sessions on other machines | from A, create a session on B with a first input; offline, then back; a retry; a refusal | — |
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
| V7 | R6n | rig: 30 notifications in 3 projects and 3 kinds, plus a DM, wake, approval and question in the same window; one arriving just after the listing, with a restart inside the cooldown; overflow past 50; Off and Poll; a remote replication gate | close-and-reopen giving two deliveries inside 10 minutes; a delayed DM; a lost item |
| V8 | R7f | conformance with injected faults: one Automation with an invalid stored definition is reported, and the others still fire and the alarm re-arms; a transient store failure during evaluation commits nothing, the retry admits the owed Firing exactly once, and nothing is marked failed; the alarm is dropped after six failures, and the next create, list or pull re-arms it and every overdue Firing is sent once; an empty journal, one create, a restart, then it fires with no further call; empty, then create, then drain, then create again; creating an earlier automation while a later deadline is armed moves the alarm earlier; a late callback with a nominal time due now but an effective time in the future doesn't send early; no callback runs between deadlines (no polling); a duplicate alarm; 250 entries split into 3 Batches with distinct IDs; same bytes; Fleet notice work not delayed | a registered but never admitted clock; double admission |
| V9 | R6 | rig: B offline for 8 days, past the notice window (`CursorExpired`), then reconnects, lists, fetches, admits and acknowledges; a lost fetch or acknowledgement reply; a crash before and after local admission; a 1 MiB instruction travels by reference; an old-release target is refused | the schedule dropped; the wake late; an oversized Batch; an old target accepted |
| V10 | R6a | conformance plus rig: pause before and after the cutoff; a lost answer | a false prevention; a chase message |
| V11 | R7 | rig with kill points: after receive; after 2 of 3 entries; after native accept | an entry lost or repeated |
| V12 | R8, R9 | rig with Haiku on a B that never held A's definitions: wake modes; a run starts with the frozen instruction after a later edit, with A offline; continuity over 2 fresh runs; a summary after B restarts mid-run | a foreign-key failure; a run stuck in Waiting; the current revision used instead of the frozen one |
| V13 | R10, R6 (U19) | conformance with a controlled clock: 250 state changes across 50 Firings arrive in ≤ 3 `ReportFirings` calls; **sparse:** 1 queued report and then silence is sent at first-queued + 500 ms (not later); 99 queued reports and then silence go at + 500 ms; the 100th report flushes immediately, before the deadline; a restart with queued reports resends them in sequence; B offline shows held; cancelled before the cutoff; native accept with its answer lost shows uncertain, then reconciled; a reporter restart keeps the sequence; reordered and duplicate reports | a regression; a reset sequence |
| V14 | R11, R12 | rig: the owner-shaped fixture, including a 60-second interval schedule and 5-minute wakes; the 10:02 one-shot upgraded at 10:03; an uncertain fresh-each-run R0 at handover (no successor until `LocalRunSettled`); R0 settling while the import is in flight (the settlement is sent only after the import is acknowledged); a not-yet-started waiting run carried into central's slot; kills before and after the local freeze, the wait for local work to finish, the central commit and the local switch | a 10:05 re-fire; two owners; an extended end; a double or missing effect |
| V15 | R13 | rig with B on the old release | a double Firing |
| V16 | R14 | local reads before and after | missing history |
| V18 | R16 | rig, **provider path** (`claude-local`, Haiku): from A, create on B with a first input; a spoofed origin; a session approver on A; B's own refusals (access, folder, model); B offline, then back; kills at admission, acknowledgement, after native create, and after input accept; partial settings; input failing after create; caller timeout. **Codex path** (`codex-local`): the same create without a first input on the rig (no model turn, per the Haiku-only rule), plus the existing Codex create-operation recorder tests for replay and unknown | a duplicate session or input; a hidden created session; a misattributed creator; the old refusal |
| V17 | R15 | docs: manifest, grep, scan over the tree and ancestry | any hit |

## Journeys needed

- `verify-codex-router`: **"central automations across two machines"** (lasting), driving V1, V3, V6, V7, V9–V12 and V14–V16 on the rig.
- **Conformance:** catalog, clock, batch, status and limits cases on both runtimes (lasting): V2–V5, V8, V10 and V13.
- **Docs publication check** (temporary): V17.
