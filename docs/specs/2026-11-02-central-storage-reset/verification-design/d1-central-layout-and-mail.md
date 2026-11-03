# Verification Design: D1, central layout and mail

How D1's requirements will be shown to hold. A plan: nothing here has been observed. Upstream: [requirements](../requirements/index.md), [specification](../specification/index.md), [program design](../program-design/d1-central-layout-and-mail.md). Journeys named are from `.agents/skills/verify-codex-router/SKILL.md`; each row says what the journey must be extended to drive.

**Targets for every row:** three debug Routers (A, B and C) and two projects (P1 held by A, P2 held by B) built from the D1 branch, each with its own debug root, enrolled against (1) a local Tokio central and (2) staging Cloudflare central. Test sessions are Luna on the debug Router only; nothing touches production or the owner's main accounts.

## Shared observations

| ID | Observation | Decides |
| --- | --- | --- |
| <a id="o1"></a>O1 | **Offline-then-online message, status read at each held stage.** B's Router stopped; a Luna session on A sends to a Luna session on B. Read A's `message show`: “accepted by Fleet”. Start B's Router with the recipient session closed, so B accepts the message into its inbox and holds it (today's held outcome for a recipient that is not ready, `mail_delivery_projection.rs:77–129`); read B's inbox row and A's `message show`: “in recipient's inbox (held)”. Open the recipient session, which releases held mail (`held_readiness.rs`); read A's `message show`: “delivered”. Each read is kept as evidence. | R7, R9, R11 |
| <a id="o2"></a>O2 | **Store listing.** After O1 on each backend, list central's stores: Tokio files under the central root; Cloudflare objects through the staging inventory (non-secret). | R1, R3 |
| <a id="o3"></a>O3 | **Crash and replay matrix.** Inject each failure in the D1 failure table (A restart before Fleet's answer; Fleet restart after admitting; B restart after accepting before the receipt; Fleet crash after receipt before status; A offline at status; C answering for B's item; hand-off failure; same id with different content; two origins forwarding to one holder with a late and a wrong-holder reply) and read every holder's records afterwards. | R9, R10, R11, R15 |
| <a id="o4"></a>O4 | **Clock-injected retention.** With an injected clock on Routers and central, read a message's records at day 59 and day 60 on every holder; submit a 61-day-old id. | R13, R14 |
| <a id="o5"></a>O5 | **Re-enrollment after an empty reset.** Wipe staging, keep URL, relay and receiver; re-enroll a Router that holds old observations; kill it mid-clearing once; write its project afterwards. | R2, R5 |
| <a id="o6"></a>O6 | **Load on one Fleet object.** On staging Cloudflare, N Routers (debug, Luna) connected at once exchanging messages at a fixed rate; record request rate, CPU per request, errors and push latency from Cloudflare analytics and Router timings. | R1 (limit) |
| <a id="o7"></a>O7 | **Project fault isolation.** Inject a failure into P1's project object (the central conformance fault seam); while it fails, A and B exchange messages and B reads and writes P2. | R3 |
| <a id="o8"></a>O8 | **Fleet schema listing.** List the Fleet store's tables on each backend. | R6 |
| <a id="o9"></a>O9 | **Same-machine send.** Two Luna sessions on A; one sends to the other; read A's inbox row (one, with UUIDv7 id and timestamps) and A's outbox (no row for it). | R8 |
| <a id="o10"></a>O10 | **Online timing on both backends.** A and B both online; 20 messages A → B; record each send-to-inbox time on Tokio and on staging. | R12 |
| <a id="o11"></a>O11 | **Unenrolled submit.** An installation D that never enrolled opens a session and submits; read the answer and the Fleet outbox count before and after. | R15 |

## Rows

| Requirement | Proven by | Reference | Target and journey | Lifetime | Passes when | Reuses | Unverified |
| --- | --- | --- | --- | --- | --- | --- | --- |
| U1 (R1, R3, R4) | O2 with three Routers and two projects; O7 | [R1](../specification/central-layout.md#r1), [R3](../specification/central-layout.md#r3), [R4](../specification/central-layout.md#r4) | Tokio and staging; Central runtime and Central staging journeys, **needed:** three-Router setup, store listing, project fault step | lasting | exactly three central stores (Fleet, P1, P2) on each backend; during P1's fault, mail and P2 keep working. A build that opens any per-machine store, or blocks mail on a project fault, fails | Board replication `board_central_e2e` for P2 read and write | capacity beyond O6's N |
| U2 (R1, R2) | O2; O1's sockets | [R2](../specification/central-layout.md#r2) | as above | lasting | no store keyed by a machine; all three sockets are held by the Fleet object | none | none |
| U3 (R5) | O5 | [R5](../specification/central-layout.md#r5) | staging; Central staging journey, **needed:** wipe and re-enroll case | temporary (cutover) | the re-enrolled Router accepts new leases and writes its project; after a kill mid-clearing it finishes clearing before any central read | none | production cutover is not in scope |
| U4 (R7, R8, R9, R10, R11, R15) | O1, O3, O9, O11 | [mail R7–R11, R15](../specification/mail.md) | Tokio and staging; Cross-machine journey, **needed:** recipient launch for B, closed recipient session for the held stage, failure matrix, unenrolled installation | lasting | B's inbox holds each message once; A's three stage reads show accepted → in inbox → delivered; every failure case ends as the failure table says; a same-machine message is in the inbox only; an unenrolled submit is refused and the Fleet outbox count is unchanged. A build that drops content before B's receipt fails the Fleet-restart case; one that jumps from accepted straight to delivered fails O1 | Cross-machine local Tokio outage/forwarding driver | Cloudflare hibernation during a failure case |
| U6, messages (R12) | O10, plus O6 under load | [R12](../specification/mail.md#r12) | Tokio and staging, both online | lasting | the send-to-inbox times on both backends are reported and contain no fixed wait | none | no latency threshold set by the owner; reported, not judged |
| U7, mail records (R13) | read every outbox and inbox row after O1 and O9 | [R13](../specification/mail.md#r13) | Tokio and staging | lasting | every message id is UUIDv7 and every row carries creation and per-holder times; a UUIDv4 id is refused | none | ids outside mail are D3 |
| U9, inbox and outbox (R14) | O4 | [R14](../specification/mail.md#r14) | Tokio and staging, injected clock | lasting | present at day 59, gone at day 60 on every holder; a 61-day-old id is refused as too old | none | a real 60-day wall-clock run is not performed |
| U11, central names (R6) | O8 | [R6](../specification/central-layout.md#r6) | Tokio and staging | lasting | every Fleet store table starts with its concern prefix (`fleet_`, `lease_`, `machine_`, `mail_`, `_fleet_sync_`, schema bookkeeping) | none | Router table names are D3 |

## Product tests the plan must add

Each failure-table case in O3 also gets a focused Rust test at its owning boundary (Fleet admission, Fleet outbox, machine inbox), using injected clocks and causal waits, never wall-clock sleeps. The library gets tests for pushed inbox acceptance, retained outbox state after content release, per-machine status, and 60-day removal.

## What stays unverified after D1

- Capacity beyond the N machines O6 drives.
- Production cutover (only staging is reset).
- Fleet-originated messages and remote create (D2).
