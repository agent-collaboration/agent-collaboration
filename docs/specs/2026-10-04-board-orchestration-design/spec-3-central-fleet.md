# Spec 3: the central fleet

**What this is:** what must be observably true of central:
- the fleet records it owns;
- the project metadata it keeps;
- each project's central copy and how it fences replication;
- the lease registry;
- the admin endpoint that moves and takes over projects;
- the one session each machine keeps with it, and what its frames carry.

It depends on [spec 1](spec-1-board-and-contracts.md) (nouns and contracts) and works alongside [spec 2](spec-2-project-storage.md) (project databases and replication as the holder sees it). It traces to [requirements.md](requirements.md); [program-design-3-central-fleet.md](program-design-3-central-fleet.md) realizes it.

```text
requirements.md ──► spec 1 (shared contracts) ──► spec 3 (this) ──► program-design-3-central-fleet.md
```

## 1. Scope

- **In:**
  - fleet records: workspaces, boards, workspace ADRs (U41), the project directory, process definitions;
  - project metadata for finding and searching projects;
  - each project's central copy (spec 2 E24): its fence, its acknowledged position, read-only reads;
  - the lease registry (E23);
  - the admin endpoint for moves and takeovers;
  - machine sessions (spec 2 E26) as central sees them, and the frames they carry (FL-12);
  - board archive across holders;
  - notices from central to Routers.
- **Out:**
  - project records, including each project's work trail and decision register (spec 1 U42, U41). They're written on the holder and reach central only as replicated state in the project's central copy (FL-2);
  - routing roles to sessions and placing sessions on machines (fabric lane P1);
  - moving sessions and folders (P1/P5);
  - the model proxy and harness connections (U29).

## 2. How it fits

```mermaid
flowchart LR
  routerA([Router on the holder machine])
  routerB([Router on another machine])
  owner([owner or admin])
  subgraph CENTRAL["Central (Cloudflare, Rust)"]
    SESSION[["machine sessions (one per machine)"]]
    COPY[("central copy, one per project: fence + acknowledged position + replicated records")]
    REG[["lease registry: holder, epoch, expiry per project"]]
    ADMIN[["admin endpoint (admin credentials only)"]]
    FLEET[("fleet records (workspaces, boards, workspace ADRs) and the project directory")]
  end
  routerA -- "hello, replication pieces, lease renewals, fleet requests" --> SESSION
  routerB -- "hello, reads, fleet requests" --> SESSION
  SESSION -- "one answer per request; notices (hints)" --> routerA
  owner -- "move, take over" --> ADMIN
  SESSION --> COPY
  SESSION --> REG
  SESSION --> FLEET
  ADMIN -- "fence first" --> COPY
  ADMIN -- "then grant" --> REG
```

## 3. Entities added here

| E | Entity | Same instance when | Belongs to / relates | Invariants | Observable states |
|---|---|---|---|---|---|
| E23 | **Lease** | (project ID, epoch) | one project; one holder, a machine installation; issued only by the lease registry: as a new project's first epoch, by a holder change (E27), or by a claim of an unheld project (FL-16) | at most one lease per project is current; epochs per project only increase, including after a restore; expiry is decided by the registry's clock; an expired lease stays with its holder until an admin changes the holder. A Router restart on the holder installation is the same holder: nothing changes the holder automatically (U39) | unheld (an imported project: no lease, no holder, no expiry; FL-15) → held by a claim (FL-16) · held → expired (the holder's writes, wakes and schedules for the project pause; reads continue) → held again on renewal (same epoch) \| superseded (by a takeover) \| released (by a move) |
| E27 | **Holder change** (a move or a takeover) | its request ID, minted by the admin's client (U10) | one project; one requesting admin; the epoch it ends and the epoch it grants | at most one holder change per project is in progress. Repeating the same request ID returns its recorded outcome and the same new epoch, and never issues another; the same ID with different content is refused as `conflictingRequest`. Recorded for the project's life with its kind, requester, reason, old and new epoch, new holder and start position | move: requested → draining → fencing → granted \| refused. Takeover: previewed → fencing → granted \| refused. A crash during fencing leaves it fencing, with the project fenced and no holder, until it's repeated (FL-10) |

## 4. Obligations (FL, plus WA-1)

- **WA-1** Fleet-level records MUST be written only by central. They are:
  - E1 and E2;
  - workspace ADRs (spec 1 E7, U41);
  - the project directory;
  - process definitions (E16);
  - leases (E23);
  - stored rejected-tail reports (FL-8).

  From a machine, these are requests that wait as pending while central is unreachable. (U20, U41; E16 is derived: MV-7's fleet-wide `name@version` uniqueness needs one writer)
- **FL-1 Project metadata.** Central MUST hold, for every project, the metadata needed to find, search and know what projects exist. Accepting a new project's directory entry is what lets its changes replicate (spec 2 WA-10). (U24)
  - The fields are the project's name, workspace and board; its status (spec 1 E3: its pinned-definition status, or closed) and its board's archive state; its holder machine and lease state (held or expired, with the expiry), or `unheld`, with no holder and no expiry (FL-15); and the time of its latest replicated record. Anything more is read from the project's central copy when needed (FL-2). (U46)
  - Search MUST find projects by text in their name and filter them by workspace, board, status and holder.
  - Every metadata result MUST say how current it is: the time each part was last updated from its source (the project's latest replicated record, or the lease registry). An offline holder's project shows its last replicated state with that time. (U46)
- **FL-2 Central reads, never writes, projects.** Central MAY read any project's central copy, read-only, for more detail. What it reads is replicated state, labelled with its feed position, never unreplicated holder state. Only the project's own replication writes its central copy, with one exception: an admin import (FL-17) writes an unheld project's initial history once, before any machine holds it, and never afterwards. That includes the project's work trail and decision register: central shows them read-only and never writes them. (U21, U24, U41, U42)
- **FL-3 Each central copy fences its project.**
  - A central copy MUST refuse, as a whole and before any of it applies, a piece from an epoch older than its fence (`staleEpoch`).
  - A fence only moves forward, and it survives restarts and objects losing their memory.
  - One project's refusal MUST NOT affect the same machine's other projects.
  - A new epoch takes effect at a central copy only between pieces, never while one is being applied. Its start position is the copy's acknowledged position at that moment.
  - Accepting a piece doesn't depend on reaching the lease registry: the fence is the check. (U21, U25, U37; spec 2 WA-3b, WA-5)
- **FL-4 A full central copy is a distinct failure.** When a project's central copy can't store more, its refusal MUST be typed `centralCopyFull`, distinct from `notWriter` and `staleEpoch`. (U32; Cloudflare's per-object storage limit)
- **FL-5 Archive is all-or-nothing to readers.** While a board is being archived, it reads as `archiving`. It reads as `archived` only once no project on it accepts work writes. Archiving freezes work, not readers: the reader opt-outs and reader state that spec 1 E2 allows (unwatch, unsubscribe, read marks and acknowledgements) are still admitted by each project's holder and still replicate. Central keeps accepting the holder's pieces for an archived project, because the holder enforces what is allowed and central can't tell one change from another. A restart resumes the archive; it never shows `archived` early. Archiving a workspace archives each of its boards this way, and the workspace reads as `archived` once all of them do. (U3, U20; review fix F13)
- **FL-6 Notices are hints.** Central MAY notify Routers that a lease was superseded or a board is archiving. Correctness never depends on a notice arriving: fencing (FL-3) and admission checks still decide. A Router that missed notices catches up from a cursor. A cursor older than retention is refused as a typed `cursorExpired`, and the Router re-reads in full. (U9)
- **FL-7 Unknown outcomes stay unknown until read.** A piece whose acknowledgement is lost has an unknown outcome until the holder reads its central copy's acknowledged position. Resending the same piece is safe (spec 2 WA-3b). A business request is never re-sent as a new request. (spec 1 RC-4)
- **FL-8 Rejected-tail reports pass through central.** A superseded holder MUST be able to report its rejected tail (project, epoch, request IDs, digest) to central. Central MUST make the report available to the project's current holder, which records the `RejectedTail` fact (spec 2 WA-5). Holders never talk to each other directly. (U11, U21)
- **FL-9 The lease registry.** The registry is the only source of project leases (E23). A lease names the project, the holder installation, the epoch and when it expires.
  - Leases last 4 hours. The holder renews every 15 minutes, and renewal is never on a write's path.
  - Expiry is decided by the registry's clock. An expired lease pauses its holder's writes, wakes and schedules for that project, and reads continue (spec 2 WA-11). It is not a holder change.
  - Renewing on the same epoch resumes the holder, unless an admin changed the holder in between. A fresh renewal attempt for an epoch that's been superseded is refused as `staleEpoch`, naming the current epoch (a re-sent attempt replays its recorded answer instead, FL-12); one from an installation that never held the project is refused as `notWriter`.
  - Epoch numbers per project only increase, including after the registry is restored from a backup. If the registry can't prove the highest epoch it has issued, it MUST refuse to issue a new one, as `registryEpochUnproven`, and change nothing.
  - A machine credential can renew its own leases, release a project when a move asks it to, reserve a new project's first epoch (spec 2 WA-10), and claim an unheld project (FL-16). It can do nothing else to a lease. (U37, U38)
- **FL-10 Moves and takeovers only through the admin endpoint.**
  - **Who:** moving or taking over a project is possible only through a separate admin endpoint, with admin credentials distinct from every machine and agent credential (the owner's own sign-in, U57). Any other credential is refused as `adminRequired` and changes nothing. By default only the owner is an admin; anyone else becomes one only when the owner grants it (derived).
  - **One at a time:** a holder change (E27) requested while another is in progress for the same project is refused as `transferInProgress`, naming the one in progress.
  - **Move:** the holder is asked to release; it stops admitting, makes every accepted record replicated, and releases (spec 2 WA-4). A move is refused as `holderUnreachable`, naming the holder's last-seen time, when the holder has no connected session or hasn't renewed in the last 30 minutes (U45); the admin may then choose a takeover. It's refused as `migrationPending` while a migration is pending or failed. When it completes, nothing has been lost.
  - **Takeover:** allowed at any time, including before the lease expires. A preview first shows the holder's last-seen time, the last replicated position and what will be voided. The takeover then needs the admin's confirmation naming that preview. If the holder, epoch or acknowledged position has changed since the preview, it's refused as `previewStale` and nothing changes. The project's central copy is fenced at the new epoch before the new holder is granted.
  - **Crash safety:** a holder change interrupted between fencing and granting leaves the project fenced with no holder. It never leaves two holders. Repeating the same request finishes it with the same epoch; it never issues another.
  - **Record:** every move and takeover is recorded as in E27. (U39)
- **FL-11 One session per machine, surviving connections.**
  - Each machine keeps one session (spec 2 E26) with central for all its projects (U34).
  - A dropped connection, a Cloudflare deploy, or central objects losing their memory MUST NOT lose or repeat anything. The session resumes from each project's acknowledged position.
  - Nothing correct depends on what a central object held in memory (U35, U36).
- **FL-13 Not on the open web (U55, U58).** Central MUST be reachable only through the owner's Access gate. It MUST refuse any request whose Access assertion fails verification, even if a credential is valid. It MUST expose no other address: no `workers.dev`, no preview URLs. A machine's session needs both its own Access service token and its machine credential. The admin endpoint needs the owner's own sign-in.
- **FL-14 Forwarding posts to the holder (U56).** A machine MAY send a post for a project it doesn't hold over its session.
  - Central MUST look up the project's current holder in the lease registry. If that holder has an open session, central MUST forward the post with the sender's verified installation, and return the holder's answer to the sender.
  - Central MUST answer `holderUnreachable` (with the holder's last-seen time) when the holder has no open session, `notHeld` when no machine holds the project, and `unanswered` when the holder doesn't answer within the forwarding bound.
  - Central keeps no post content and no outcome. It holds only a short-lived correlation (forward ID, the sender's connection and frame, the expected holder and a deadline), so the answer reaches the right sender and only from that holder. It drops the correlation at the answer or the deadline. The holder's ledger is the only record of the post, so a retry is safe.
  - When the sender itself holds the project, central answers `senderHolds` and the post is admitted locally.
  - Central MUST forward posts only. Every other project write from a non-holder is refused `notWriter` without being forwarded.
  - A machine has a bounded number of forwards in flight; beyond that, it's answered `overloaded`.
- **FL-15 Unheld projects (U54).** A project can exist with a central copy and no holder: `unheld`. Only an admin import (FL-17) creates one. Its copy is readable (read copies, directory and metadata). A write needs a holder: it's refused `notHeld`, and a post forwarded to it is answered `notHeld` (FL-14).
- **FL-16 Claiming an unheld project.** A machine MAY claim an unheld project with its machine credential; no admin is needed. The registry MUST grant the claimant the next epoch, after the epoch floor. The machine builds its store from the copy at the copy's acknowledged position before admitting anything, as a moved project's new holder does (spec 2 WA-4). A claim on a held project is refused `alreadyHeld`, naming the holder, and changes nothing; moves and takeovers stay admin-only (FL-10). Of two racing claims, exactly one wins; the other gets `alreadyHeld`. A re-sent claim with the same attempt ID gets its recorded answer and the same epoch, never a second one, including after a crash between the copy's fence and the grant. A claim from a machine whose Router build can't support the copy's schema is refused `schemaTooNew`, naming both, and changes nothing. A copy whose fence has moved past its import (it was claimed before, even if the registry has since been restored) is never granted again by a claim: the answer is `alreadyHeld`, naming that epoch's holder.
- **FL-17 Admin import (U54).** Only the owner's admin sign-in (U57) may import.
  - An import installs fleet records (workspaces, boards and directory entries) with their original IDs and creation times, plus each project's complete history as an `unheld` copy whose acknowledged position is the imported head.
  - Re-importing an identical project, by ID and digest, is answered `alreadyImported` and changes nothing, whether or not it has been claimed since. Different content under an existing ID is refused `importConflict`, naming both digests, or naming the offered one and saying the existing project wasn't created by an import.
  - An import never touches a project that is held or has ever been claimed.
  - Imported history belongs to the import epoch, epoch 1, which has no holder. The first claim gets the next epoch (FL-16).
- **FL-12 What a session carries.** These are the observable rules for every frame on a machine session. The concrete frame types, their encoding and the transport are Program Design.
  - **Opening and credentials.** A machine opens or resumes its session with a hello that presents its machine credential. A machine credential proves one machine installation and nothing more.
    - Central MUST accept the hello only while that credential is current for that installation. Otherwise it answers `credentialRejected`, naming why (unknown, expired, revoked, or not a machine credential), and closes the session. Agent and admin credentials can't open a session.
    - A credential that stops being current mid-session gets `credentialRejected` on its next request, and the session closes. The machine opens a new one with a current credential.
    - An accepted hello is answered with, for each project the machine holds or replicates, its acknowledged position and its lease (epoch, expiry, held or expired), so resuming starts from there.
    - The logical session is not the connection (U36). A newer connection from the same installation takes over its session; the older connection is closed as `connectionReplaced`, and requests in flight on it are re-sent on the new one. A hello naming a different session ID starts a new logical session for that installation and ends the old one; nothing is lost, because acknowledged positions belong to each project's central copy. (U34, U49)
  - **What a machine credential may do:** send replication pieces (the fence decides, FL-3), renew its own leases, release a project a move asked it to, reserve a new project's first epoch, claim an unheld project (FL-16), report its own rejected tails, and send fleet requests and reads for the actors on that machine. Those actors are recorded as declared (spec 1 §6). It can't move or take over a project (`adminRequired`, FL-10). Admin credentials are used only at the admin endpoint, never on a session.
  - **Requests and answers.** Every frame a machine sends that expects an answer carries an ID unique within its session. Central answers each one exactly once, naming that ID, with the result or a typed failure from spec 1 §5. Answers may arrive in any order.
    - An answer lost with its connection leaves the outcome unknown. After resuming, the machine re-sends the request, and re-sending never repeats an effect: a piece is matched by its identity and digest (spec 2 E25), a renewal by its attempt ID (each scheduled renewal is a new attempt with a new ID, and a re-sent attempt keeps its ID and doesn't renew again), a release by the holder change it answers, a fleet request by its request ID (spec 1 E21, RC-4), and a read has no effect.

    | Request | Names | Its answer carries |
    |---|---|---|
    | Replication piece | its identity and digest (E25) | the acknowledged position, or `staleEpoch`, `positionGap` (naming the acknowledged position), `pieceConflict`, `notInitialized` or `centralCopyFull` |
    | Lease renewal | its attempt ID, project, epoch | `renewed` with the new expiry (a re-sent attempt gets its recorded result, marked as a replay, plus the lease's current state), and any holder change in progress for the project; or `staleEpoch` or `notWriter` |
    | Release | project, epoch, the holder change asking for it | released, with the final acknowledged position; `positionGap` while the copy hasn't acknowledged everything; or `releaseRefused` naming the holder change and its reason: `migrationPending` (the holder's pending or failed migration) or `schemaTooNew` (the copy's fence refusing the target schema). A re-sent release gets the same answer |
    | First-epoch reservation | the new project | its first lease, or `projectRejected` naming central's reason |
    | Claim | its attempt ID and the project | `claimed` with the granted epoch, its expiry and the copy's acknowledged position to seed from (a re-sent attempt gets its recorded answer), `alreadyHeld` naming the holder, `schemaTooNew` naming both versions, or `notFound` |
    | Rejected-tail report | project, epoch, request IDs, digest | stored, or `notWriter` if that epoch wasn't this installation's |
    | Forwarded post | project, post kind and the post itself (its own request ID inside) | the holder's answer, or `senderHolds`, `notHeld`, `holderUnreachable` (with the holder's last-seen time), `unanswered` or `overloaded` |
    | Fleet request | its request (E21) | its decision (spec 1 RC-4): accepted with the records and their fleet feed positions, or rejected with reasons |
    | Read | a scope (fleet records, the project directory and metadata, one project's central copy, or the machine's notices) and an optional cursor | the records, the position they reflect (a fleet feed position, a project's acknowledged replication position, or a notice position) and when that position was current; `cursorExpired` for a cursor older than retention |

  - **The renewal answer is authoritative.** A holder learns that its lease renewed, expired or was superseded from the answer to its renewal, never from a notice. A re-sent attempt that was already decided gets its recorded result again, marked as a replay, together with the lease's state now. The replay never renews again and never restores authority: the holder acts on the current state, so if a takeover happened in between, it learns it's superseded and stops writing. That answer also names any holder change in progress, so a holder that missed the notice asking it to release learns of the move at its next renewal (derived from FL-6).
  - **Notices.** A notice names its kind (lease superseded, lease granted, release requested, board archiving, rejected tail reported), its scope (a project with its epoch, or a board) and its position in that machine's notice feed. A notice changes nothing by itself; the machine acts only on the answer to a request (FL-6). It catches up by reading its notices after the last position it has. (U9, U34, U36, U39)

## 5. Proof

| V | Proves | Evidence |
|---|---|---|
| V7 | WA-1, FL-3, FL-10, whole-unit moves across machines | Integration gates before the fleet counts as done: <br>• fleet requests from a second machine while central is unreachable, and then once it's back; <br>• a second machine's write into a project refused, except a post, which is forwarded to the holder and admitted there (WA-13); <br>• a post forwarded between two real machines through central, answered from its holder; <br>• admin credentials required for a move and for a takeover; <br>• a takeover while a piece is in flight; <br>• moving a whole project unit between real machines. <br>Unverified until P1 and P5 exist |
| V9 | FL-1…FL-17 on one machine | The real Rust central under the local Cloudflare runtime, with two holder stores. Each of these must be shown: <br>• a takeover that fences between pieces and starts the new epoch at the acknowledged position; <br>• a stale piece refused for one project while another project's piece from the same machine applies; <br>• a fence that survives an object restart; <br>• a typed `centralCopyFull`; <br>• a superseded holder's rejected-tail report reaching the current holder through central; <br>• metadata listing and searching projects with their freshness; <br>• read-only reads returning replicated state; <br>• archive showing `archiving` until every project has stopped writing, including across a restart; <br>• a missed notice recovered from its cursor; <br>• a piece with a lost acknowledgement, reconciled and safely resent; <br>• expiry and renewal on the registry's clock; <br>• `adminRequired` for a machine credential; <br>• a takeover interrupted between fencing and granting, finished by repeating it, with no second epoch; <br>• a refused grant when the registry can't prove its highest epoch; <br>• the session resuming after a dropped connection and after objects lose their memory; <br>• `transferInProgress`, `holderUnreachable`, `migrationPending` and `previewStale` for holder changes, and a repeated holder-change request returning the same epoch; <br>• `credentialRejected` for an unknown, expired, revoked, agent or admin credential at hello, and mid-session; <br>• `connectionReplaced` for the older of two connections from one installation, with its in-flight requests re-sent on the newer one; <br>• a renewal R1 accepted with its answer lost, re-sent after a connection replacement (no second renewal; same expiry returned), then the next scheduled renewal R2 renewing again; <br>• R1 renewed with its answer lost, then a takeover to a new epoch, then R1 re-sent (its recorded success marked as a replay, with the current state showing the lease superseded, and no writes resumed), then a fresh R2 for the old epoch refused as `staleEpoch`; <br>• every request kind answered once with its ID, including a lost answer re-sent after resuming with no repeated effect; <br>• a holder that missed the release notice learning of the move from its renewal answer; <br>• workspace ADRs written only through central, and a project's work trail and decision register readable from its central copy and never written there; <br>• **FL-13:** a request with no Access assertion, a bad signature, the wrong `aud` or issuer, or an expired one, refused at the edge even with a valid machine credential; a session whose Access client ID and machine credential name different installations refused; a session assertion refused on the admin route and an owner assertion on the session route; a revoked installation's open session closed; <br>• **FL-14:** a forwarded post admitted by its holder and answered to the sender; an answer from a peer that wasn't sent the post dropped; `holderUnreachable` with no holder session; `senderHolds`; `notHeld`; the deadline giving `unanswered`, with a retry by the same request ID replaying the holder's decision; the in-flight cap giving `overloaded`, including two forwards racing for the last slot while their route lookups are pending, and slots freed on `senderHolds`, `notHeld`, an answer and the deadline; a sender object restart giving `unanswered`; <br>• **FL-15 to FL-17:** an imported project is unheld: its copy is readable, a write is refused `notHeld` and a forwarded post is answered `notHeld`; a claim grants the epoch after the floor, and the claimant's store is built from the copy at its acknowledged position before anything is admitted; of two racing claims exactly one wins and the other gets `alreadyHeld`; a re-sent claim replays its answer, including after a crash between the copy's fence and the grant, with the same epoch; a claim after the registry is restored behind an already claimed copy gets `alreadyHeld` and no new epoch; a claim from a build that can't support the copy's schema gets `schemaTooNew`; the claimant's first piece applies at the imported head; a claim on a held project is refused `alreadyHeld` and changes nothing; an import with a machine credential is refused at the edge; a re-import with the same digest is answered `alreadyImported`, and different content `importConflict` naming both digests; an import over a held or once-claimed project is refused |
| V9d | FL-3, FL-10, FL-11, FL-13 on Cloudflare | The fence, the takeover crash cases and session resume against a deployed staging account, including a deploy mid-session. For FL-13: real Access-issued assertions verified by signature (a decoded-only assertion doesn't count), service tokens on the WebSocket upgrade, the owner's sign-in on the admin route, and no reachable `workers.dev` or preview address. Until it runs, these are proved only locally (spec 2 G18) |

## 6. Coverage

| U | Entities | Obligations | Proof |
|---|---|---|---|
| U20, U41 | E1, E2, E7 (workspace ADRs), E16, E23 | WA-1 | V7, V9 |
| U24, U41, U42, U46 | E3, E24, E29 (held in E24 as replicated project records) | FL-1, FL-2 | V9 |
| U21, U25, U37 | E20, E24 | FL-3, FL-4 | V9, V9d |
| U3 | E2 | FL-5 | V9 |
| U9 | — | FL-6, FL-7 | V9 |
| U11 | E22 | FL-8 | V9 |
| U37, U38 | E23 | FL-9 | V9 |
| U39, U45 | E23, E27 | FL-10 | V7, V9, V9d |
| U34, U35, U36, U49 | E26 | FL-11, FL-12 (session, frames, answers, notices) | V9, V9d |
| U37, U38, U39 | E23, E27 | FL-12 (renewal answer, release, first-epoch reservation, rejected-tail report, machine versus admin credentials) | V9 |
| U32 | E24, E23 | central runs on Cloudflare (WA-9 in spec 2) | V9, V9d |
| U55, U57, U58 | E26 | FL-13 (Access gate, machine and owner routes) | V9, V9d |
| U56 | E26, E10 | FL-14 (forwarded posts) | V7, V9 |
| U54, U57 | E1, E2, E3, E23, E24 | FL-15, FL-16, FL-17 (unheld projects, claim, admin import) | V4, V9 |

## 7. Gaps

| G | Gap | Owner |
|---|---|---|
| G4 | Who builds the lease registry and the admin endpoint. **Settled by U53:** this lane, with a separate verification Sidekick proving it on real paths | owner, 2026-10-13 |
| G20 | Where the admin credential lives. **Settled (U57):** there's no stored admin secret; admin is the owner's Google sign-in through Access | owner, 2026-10-13 |
| G26 | How only the owner's computers reach central (U55). **Settled (U58):** Cloudflare Access on a dedicated hostname. Each machine has its own Access service token plus its machine credential, and central verifies the Access assertion itself (FL-13). Verified on the owner's account on 2026-10-13 | owner, 2026-10-13 |
| G21 | Settled by U50: the Workers Paid plan (10 GB per object bounds `centralCopyFull`, FL-4). Still needed from the owner: the staging account and a deploy token in 1Password | owner (account and token) |
