# Verification Design: D2, Fleet sends and remote create

A plan: nothing here has been observed. Upstream: [R16–R22](../specification/remote-sessions.md), [program design](../program-design/d2-fleet-sends-and-remote-create.md). Targets: the D1 three-Router harness (A, B, C) on Tokio central and staging central; Luna sessions through the owner's debug Router proxy.

| Obs. | Observation | Decides |
| --- | --- | --- |
| P1 | B stopped; from a session on A request a session on B with a first input; read A's remote-create status (“accepted”, then “waiting for B”); start B; with a conformance pause in B's command owner after inbox acceptance and before the provider create, read status (“B has your request”); release the pause; read the result message in the requesting session; message the new session and read its reply | R16, R18, R20 |
| P2 | Push the same create command to B twice (conformance replay control); count sessions on B | R19 |
| P3 | Crash at each named point: B after the provider create and before recording it; B after the prompt and before the report; the Fleet after applying the report (the result must still arrive once). Restart; read reports, sessions, prompts and result messages | R19, R20 |
| P8 | Same request id with a different command; a request whose deadline passes with B offline; a create with partial settings and a first input | R16, R18, R20 |
| P4 | Request from never-enrolled installation D, and to a target that is not enrolled; read the answers and the Fleet outbox count | R16 |
| P5 | Submit an item with sender `Fleet` from machine A over its socket; read the refusal | R21 |
| P6 | In the new session on B, trigger a tool call that needs approval; read B's owner approval list | R22 |
| P7 | Read the temporary-rule code comment and its test name | R17 |

| Requirement | Proven by | Passes when | Unverified |
| --- | --- | --- | --- |
| U10 (R16–R20, R22) | P1–P4, P6–P8 | one session per request; stages in order; result exactly once; first input at most once, with “unknown” when B cannot tell; refusals store nothing; approvals on B; a build that creates twice on replay or after a crash fails P2/P3 | real Claude and Cursor endpoints beyond the first one driven |
| U8 (R21) | P1, P5 | the recipient shows the Fleet as sender; a machine cannot submit as the Fleet | Fleet messages from automations (D4) |

P1's agent reply needs a model turn, so it needs the debug Router running.
