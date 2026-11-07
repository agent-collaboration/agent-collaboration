# Verification Design: D2, Fleet sends and remote create

A plan: nothing here has been observed. Upstream: [R16–R22](../specification/remote-sessions.md), [program design](../program-design/d2-fleet-sends-and-remote-create.md). Targets: the D1 three-Router harness (A, B, C) on Tokio central and staging central; Luna sessions through the owner's debug Router proxy.

| Obs. | Observation | Decides |
| --- | --- | --- |
| P1 | B stopped: request a session on B and read the refusal “B is offline”, with the Fleet outbox unchanged (temporary rule). Start B; from a session on A request a session on B with a first input; read A's remote-create status (“accepted”); with a conformance pause in B's command owner after inbox acceptance and before the provider create, read status (“B has your request”); release the pause; read the result message in the requesting session; message the new session and read its reply | R16, R18, R20 |
| P2 | Push the same create command to B twice (conformance replay control); count sessions on B | R19 |
| P3 | Crash at each named point: B after the provider create and before recording it; B after the prompt and before the report; the Fleet after applying the report (the result must still arrive once). Restart; read reports, sessions, prompts and result messages | R19, R20 |
| P8 | Same request id with a different command; a command still undelivered at the 60-day removal (injected clock) with B offline; a create with partial settings and a first input | R16, R18, R20 |
| P4 | Request from never-enrolled installation D, and to a target that is not enrolled; read the answers and the Fleet outbox count | R16 |
| P5 | Submit an item with sender `Fleet` from machine A over its socket; read the refusal | R21 |
| P6 | In the new session on B, trigger a tool call that needs approval; read B's owner approval list | R22 |
| P7 | Read the temporary-rule code comment and its test name | R17 |

| Requirement | Proven by | Passes when | Unverified |
| --- | --- | --- | --- |
| U10 (R16–R20, R22) | P1–P4, P6–P8 | one session per request; stages in order; result exactly once; first input at most once, with “unknown” when B cannot tell; refusals store nothing; approvals on B; a build that creates twice on replay or after a crash fails P2/P3 | real Claude and Cursor endpoints beyond the first one driven |
| U8 (R21) | P1, P5 | the recipient shows the Fleet as sender; a machine cannot submit as the Fleet | Fleet messages from automations (D4) |

P1's agent reply needs a model turn, so it needs the debug Router running.

## Revision r2 (2026-11-05, review F8)

**Entries used.** The requesting side drives the real CLI `agent-collaboration conversation create --machine <B> …` and `conversation remote-status --request-id …`, plus the MCP `conversation_create` with `machine` (one MCP case: P1's accepted path). The result is read from the requesting session's inbox through `message inbox` and `show`. Reads stay public, except the conformance pause and the injected clock.

**Both create owners.** P1 runs once with `codex-local` and once with one external provider (`claude-local`). P6 observes the approver on B for each: the target's named Chief of Staff session. Unverified beyond those two: Cursor and other providers.

**Added negative cases and readbacks.**

| Obs. | Observation | Decides |
| --- | --- | --- |
| P9 | A remote create when B has no named Chief of Staff: refused `NoRemoteApprover`, no session on B, one result message carrying the refusal | R22 (interim approver) |
| P10 | A caller (CLI, MCP or socket) supplying a remote-requester actor or a `Machine` address with a non-Fleet sender: refused, Fleet outbox unchanged | R21, F4/F6 |
| P11 | Record readback after P1, P3 and P8: `fleet_remote_creates` (result id null until the outcome, then set once), `mail_create_commands` (prompt `dispatching` → `accepted`; after a crash, `unknown` and no second prompt), and the result item's creation time later than the command's | R18, R19, R20 (F2, F5) |
| P12 | Retained D1 session mail still delivers after the D2 upgrade (an old bare-`SessionRef` address row decodes as `Session`) | R7–R11 continuity (F6) |

**Targets.** All P cases on Tokio central. On staging Cloudflare, after the D2 redeploy through the fleet-ops D1 redeploy mode: P1 (codex-local), P4, P5 and P10. Local Tokio results aren't counted as Cloudflare proof. Crash points (P3) stay native-only, matching D1's accepted boundary.


## Revision r3 (review F8 residual)

| Obs. | Observation | Decides | A wrong build it catches |
|---|---|---|---|
| P13 | First input that is definitely refused before delivery (an invalid input for the endpoint): the result is `Created` with `first_input: Failed { Refused }`, and no prompt reaches the session | R20 | always reports `accepted` |
| P14 | A offline when the result materializes: (a) the normal result; (b) late expiry. Keep A offline, advance past the command's deadline so the expired result is minted then, reopen the Fleet, reconnect A, and read exactly one result whose retention starts at its own creation (its UUIDv7 time later than the command's deadline) | R18 (F2) | discards a result while A is disconnected; mints the result id early |
| P15 | Four cases on one request id. (a) An unrelated B-local operation with that id is pre-seeded and no `mail_create_commands` row exists: `Refused { ProviderRefused { code: "operation_conflict" } }`, the existing operation and its session untouched, no prompt. (b) A matching `pending` row is staged, then an unrelated B-local create admits and completes that id before the command resumes: `Unknown`, the unrelated operation and its session unchanged, no first input. (c) A `pending` row whose provider has no operation: exactly one create, from that admission's own answer. (d) A completed row with the same digest: the replay answers from the row and the provider is not called | R19 | adopts an operation it didn't admit (`conversation_operations.rs:253`, `external_provider_supervisor.rs:279`); trusts a pending row as proof of ownership |
| P8b | Same id and unchanged inner command, but a changed target or requester: `Conflict`, the original row, command and result unchanged | R16 (F10) | digest covers the command only |
| P11b | P11's readback also asserts every new row's UUIDv7 key and its `created_at`/`updated_at` validity | U7/R13 | non-UUIDv7 keys, missing timestamps |

**Harness extensions needed** (none exist yet; the D1 harness `verification/observations/central/d1-harness.ts` is the foundation, with approvals off and `stagingExecuted: false`): an approval-enabled B with a named remote-create approver; an A disconnect and reconnect around result materialization; the D1 injected mail clock for the 60-day deadline; and a B-local pre-seeded operation and a staged pending command row for P15. The staging subset adds P13 to P1, P4, P5 and P10. Local Tokio stays non-Cloudflare proof.