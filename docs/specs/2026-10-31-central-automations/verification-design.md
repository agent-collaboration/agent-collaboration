# Verification Design: central automations

This is the plan for proving R1–R15. Nothing in it has been observed.

## Targets

- **Fleet rig:** two isolated, signed debug Routers (machine A and machine B) enrolled with a local **Tokio central** (`collaboration-central-tokio`), on Sunclaw. Real agent sessions use `claude-haiku-5-5` with a small budget.
- **Both runtimes:** the central behaviour also runs through the `collaboration-central-conformance` suite on Cloudflare and Tokio (C2).
- **Docs:** the published branch itself, for R15.

## "Proven by", for the owner

| Promise | Proven by | Stays unverified |
| --- | --- | --- |
| One list everywhere | Create on A; B lists and shows it with the same state; pause on B; A sees it | Many machines at once (two are used) |
| Not tied to a project | Expire every lease; the automation still fires on time | — |
| Wake limits | A second wake for one session from the other machine is refused with its creator named; each limit refuses | — |
| Central fires | Stop A (the creator); A's schedule still runs on B on time | Long clock drift |
| Offline target | Stop B; the firing is held; restart B; it's delivered once; a wake whose end passes while held ends as expired | — |
| Once, never twice | Force redelivery of one firing; one message, one run | — |
| Runs as today | The wake push header and kind match today's; the scheduled run starts in the bound or fresh session and records its summary | — |
| Outcome everywhere | Show from A lists B's run states in order | — |
| Migration | A holds the owner-shaped fixture (9 schedules, active and paused wakes); upgrade; identities and next due times are equal; a due time during the move fires once; kill and restart mid-move makes no duplicate | The owner's real data, checked by the owner after upgrade |
| Old machines keep working | B on the old release keeps firing its own; central never fires them | — |
| Published design | The manifest diff (newer main docs untouched); grep that no governing doc carries the pause, lost-write or offline-project obligations; the public-safety scan of staged paths and ancestry | Semantic review is by people, not grep |

## Coverage rows

| U | R | Observation | Passes when | Wrong build that fails |
| --- | --- | --- | --- | --- |
| U2 | R1 | rig: CLI on A and B, JSON compared | equal lists and states after an acknowledged write | a per-machine list; a stale read |
| U3 | R2 | rig + conformance: leases forced expired | the Firing at its due time | any lease gate |
| U4 | R3 | conformance + rig | the typed refusals named | a second active Wake accepted |
| U2 | R4 | rig with central stopped | `centralUnreachable`; no row after central restarts | a write queued and later applied |
| U5 | R5 | rig: A stopped | B gets a Firing at its due time; fold rules hold | nothing fires; two waiting runs |
| U6 | R6 | rig: B stopped, then restarted | held, then one delivery; expired after `valid_until` | dropped; delivered twice; delivered late |
| U7 | R6a | conformance with an injected race | cancelled with no injection; the new revision governs | delivery after cancel |
| U7 | R7 | rig: forced redelivery | one `firing_receipts` row, one push or run | two runs |
| U7 | R8, R9 | rig with Haiku sessions | header and kind equal to today's; the run is in the right session with its summary | a wrong session or header |
| U8 | R10 | rig: Show from A for a run on B | ordered states, no regression | a missing state; an older state shown |
| U9 | R11, R12 | rig: fixture plus a kill mid-move | equal IDs and due times; one Firing per due time | lost, duplicated, or double-fired |
| U9 | R13 | rig: B on the old release | B fires locally; the Fleet has no rows for B | a double Firing |
| U10 | R14 | rig: local history before and after | identical reads | history missing before retention |
| U11 | R15 | docs: manifest, grep, scan | all three clean | any contradiction, deletion or hit |

## Journeys needed

- `verify-codex-router`: **"central automations across two machines"** (lasting). It drives the rig through R1–R13. It's new and built by the Verifier.
- **Conformance:** new automation catalog and clock cases on both runtimes (lasting).
- **Docs publication check** (temporary): a script over the staged tree.
