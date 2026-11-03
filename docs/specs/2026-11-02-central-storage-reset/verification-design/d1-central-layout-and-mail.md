# Verification Design: D1, central layout and mail

How D1's requirements will be shown to hold. A plan: nothing here has been observed. Upstream: [requirements](../requirements/index.md), [specification](../specification/index.md), [program design](../program-design/d1-central-layout-and-mail.md). Journeys named are from `.agents/skills/verify-codex-router/SKILL.md`; each row says what the journey must be extended to drive.

**Targets for every row:** two debug Routers (A and B) built from the D1 branch, each with its own debug root, enrolled against (1) a local Tokio central and (2) staging Cloudflare central. Test sessions are Luna on the debug Router only; nothing touches production or the owner's main accounts.

## Shared observations

| ID | Observation | Decides |
| --- | --- | --- |
| <a id="o1"></a>O1 | **Offline-then-online message.** B's Router stopped; a Luna session on A sends to a Luna session on B; read A's outbox record and Fleet's outbox record; start B; read B's inbox and the session's received message; read A's `message show` status history. | R7, R9, R11, R12 |
| <a id="o2"></a>O2 | **Store listing.** After O1 on each backend, list central's stores: Tokio files under the central root; Cloudflare objects through the staging inventory (non-secret). | R1, R3 |
| <a id="o3"></a>O3 | **Crash and replay matrix.** Inject each failure in the D1 failure table (A restart before Fleet's answer; Fleet restart after admitting; B restart after accepting before the receipt; Fleet crash after receipt before status; A offline at status; C answering for B's item; hand-off failure; same id with different content; two origins forwarding to one holder with a late and a wrong-holder reply) and read every holder's records afterwards. | R9, R10, R11, R15 |
| <a id="o4"></a>O4 | **Clock-injected retention.** With an injected clock on Routers and central, read a message's records at day 59 and day 60 on every holder; submit a 61-day-old id. | R13, R14 |
| <a id="o5"></a>O5 | **Re-enrollment after an empty reset.** Wipe staging, keep URL, relay and receiver; re-enroll a Router that holds old observations; kill it mid-clearing once; write its project afterwards. | R2, R5 |
| <a id="o6"></a>O6 | **Load on one Fleet object.** On staging Cloudflare, N Routers (debug, Luna) connected at once exchanging messages at a fixed rate; record request rate, CPU per request, errors and push latency from Cloudflare analytics and Router timings. | R1 (limit), R12 |

## Rows

| Requirement | Proven by | Reference | Target and journey | Lifetime | Passes when | Reuses | Unverified |
| --- | --- | --- | --- | --- | --- | --- | --- |
| U1 (R1, R3, R4) | O1 and O2 on both backends show one Fleet store plus project stores, and messages and projects work | [R1](../specification/central-layout.md#r1), [R3](../specification/central-layout.md#r3), [R4](../specification/central-layout.md#r4) | Tokio and staging; Central runtime and Central staging journeys, **needed:** a store-listing step and the two-Router message case | lasting | exactly one Fleet store and one store per project on each backend; a build that still opens `connection-*` or `mailbox-*` fails | Board replication `board_central_e2e` for project read/write after re-pointing | real workload capacity beyond O6's N |
| U2 (R1, R2) | O2 shows no per-machine store; O1's two connections both terminate on the Fleet | [R2](../specification/central-layout.md#r2) | as above | lasting | no store keyed by a machine exists; both sockets are held by the Fleet object | none | none |
| U3 (R5) | O5 | [R5](../specification/central-layout.md#r5) | staging; Central staging journey, **needed:** wipe + re-enroll case | temporary (cutover) | the re-enrolled Router accepts new leases and writes its project; after a kill mid-clearing it finishes clearing before any central read | none | production cutover is not in scope |
| U4 (R7, R8, R9, R10, R11, R15) | O1 and O3 | [mail R7–R11, R15](../specification/mail.md) | Tokio and staging; Cross-machine journey, **needed:** recipient-route launch for B and the failure matrix | lasting | B's inbox holds each message once; status steps arrive in order; every failure case ends as the D1 failure table says; a C-forged receipt changes nothing; a build that drops content before B's receipt fails the Fleet-restart case | Cross-machine local Tokio outage/forwarding driver | Cloudflare hibernation during a failure case |
| U6, messages (R12) | O1 and O6 measure send-to-inbox time with both online | [R12](../specification/mail.md#r12) | Tokio and staging | lasting | no batching delay is added: the median send-to-inbox time is reported and has no fixed wait in it | none | owner has set no latency threshold; the number is reported, not judged |
| U7, mail records (R13) | Read every outbox and inbox row after O1 | [R13](../specification/mail.md#r13) | Tokio and staging | lasting | every message id is UUIDv7, every row carries creation and per-holder times; a UUIDv4 id is refused | none | ids outside mail are D3 |
| U9, inbox and outbox (R14) | O4 | [R14](../specification/mail.md#r14) | Tokio (injected clock); staging with the same clock seam | lasting | present at day 59, gone at day 60 on every holder; a 61-day-old id is refused as too old | none | real wall-clock run of 60 days is not performed |

## Product tests the plan must add

Each failure-table case in O3 also gets a focused Rust test at its owning boundary (Fleet admission, Fleet outbox, machine inbox), using injected clocks and causal waits, never wall-clock sleeps. The library gets tests for pushed inbox acceptance, retained outbox state after content release, per-machine status, and 60-day removal.

## What stays unverified after D1

- Capacity beyond the N machines O6 drives.
- Production cutover (only staging is reset).
- Fleet-originated messages and remote create (D2).
