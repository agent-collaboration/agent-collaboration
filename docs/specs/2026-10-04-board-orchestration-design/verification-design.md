# Verification design: the board and central on agent-fleet-sync

**What this is:** the plan for showing that the board's requirements hold now that the board and central consume agent-fleet-sync (U78) with a new board schema (U79).

**It's a plan.** Nothing in it has been observed. Each row records what will be done, on which target, what will be seen, which wrong build fails it, and what stays open.

```text
requirements.md ──► spec 1, spec 2, spec 3, one-core spec ──► PD1, PD2, PD3 (revision map), PD4 (revision map), one-core PD ──► this
```

| Document | Identity |
|---|---|
| Requirements | [requirements.md](requirements.md), U1–U79 |
| Specifications | [spec 1](spec-1-board-and-contracts.md), [spec 2](spec-2-project-storage.md), [spec 3](spec-3-central-fleet.md), [one-core spec](../2026-10-16-central-one-core/2026-10-16-central-one-core.md) |
| Program Designs | [PD1](program-design-1-board.md), [PD2](program-design-2-project-storage.md), [PD3](program-design-3-central-fleet.md) and [PD4](program-design-4-messaging.md) (each with its 2026-10-17 revision map), [one-core PD](../2026-10-16-central-one-core/program-design.md) (central's governing design) |
| Verification Design | this file |
| Its dependency | [agent-fleet-sync consumer-fit VD](../2026-10-16-agent-fleet-sync/verification-design.md) (SO51–SO57). Rows here that rest on a library change name it |

Seams come from the trace tables: PD1 §11, PD2 §9, PD4 §11 and one-core §13.

## 1. Targets

Every target is a planned build, not one anyone has seen.

| Target | Build and configuration |
|---|---|
| **T-local-tokio** | Two isolated signed debug Router installations, A and B, on one Mac. Each has its own debug root and Host (`verify-codex-router` launch rules; normal Codex home untouched; production Router untouched). Both run against central on its Tokio runtime (`collaboration-central-tokio`), with its own data directory. agent-fleet-sync is pinned at the W-C tag |
| **T-local-workerd** | The same two Routers against central under local workerd (the pinned Miniflare and workerd versions) |
| **T-staging** | The two Routers against central deployed to the staging Cloudflare account. It sits on the owner's dedicated domain behind Access, with a per-machine Access service token plus machine credential, and the owner's Google sign-in for admin routes (U55, U57, U58) |
| **T-two-machines** | One isolated debug Router on each of two development machines, both against T-staging |
| **T-legacy-copies** | Read-only copies of the owner's real `project-board.sqlite` files, never the live stores, plus X2's constructed fixtures. The upgrade binary runs against T-local-tokio's central |

## 2. Journeys

**Read:** `.agents/skills/verify-codex-router/SKILL.md`. Its journeys are stored native push, typed provider callbacks, owning SQLite reopen and the draft private Tailnet journey. None of them drives a board write through replication, central, a holder change or the upgrade. So every journey below is `needed:`.

Each is built as a self-describing script under `scripts/verification/` and indexed in `verify-codex-router` before any observation that depends on it.

| Journey | Lifetime | Entry it drives | What it reads |
|---|---|---|---|
| `needed: board-records` | lasting | the collaboration API over MCP (TCP) and the CLI (Unix socket) of one Router holding a project | each answer; views; the project database at named checkpoints, by the store readback route below |
| `needed: board-replication` | lasting | board writes on A; session and central faults; Router and central restarts | A's views and project database (`capture_feed_marks`); central's copy through the owner operations view and through a read copy on B |
| `needed: holder-change` | lasting | owner admin routes (move, takeover preview and confirm, cancel, archive, retire) and the Router's claim tool | every answer in board names; the transfer record through the operations view; the copy's fence; each Router's writer state, views, facts and files |
| `needed: cross-machine` | lasting | posts, messages, notifications, cancels and wakes between two machines | what each recipient session actually receives; sender statuses; the holder's ledger |
| `needed: central-runtime` | lasting | central process and object faults on both local runtimes: restart, a dropped HTTP response, a held storage operation, a forged caller | durable state through the operations view and, on Tokio, each object's store by the stop-then-reopen route (§2); the responses |
| `needed: central-staging` | lasting (release check) | the deployed edge: Access, sessions, a deploy mid-session, idle eviction | the same as above, through the deployed edge only |
| `needed: board-upgrade` | temporary: the upgrade runs once, at the release | `codex-router-board-upgrade` (`--dry-run`, real run, interrupted runs, rerun) | its JSON report; each new project database; central's `admin_imports`; X2's oracle |

**The store readback route.** No concurrent external read-only open of a live Turso store has been qualified, so no journey assumes one. Stored state is read at named checkpoints through one of two routes:
- **through the owning process:** the Router's own read path, the central operations view, or a read copy;
- **stop, then reopen:** follow `verify-codex-router`'s owning-reopen discipline. Wait until every owned operation, caller and observer has settled, close the owning Router or central process, open the file read-only, then restart on the same root and binary.

A journey that needs a concurrent read path has to qualify it first. Until then, that path is unverified.

The consumer-fit journeys in agent-fleet-sync's VD (`holder-refusals`, `target-capability`, `frame-shapes`, `copy-containment`) are prerequisites for the rows that rest on SO51–SO57.

## 3. Shared observations

Each is described once. The rows below name the parts they decide.

**OB1: board records on the new schema** (`board-records`, T-local-tokio).
- **Does:** runs spec 1's V1, V2, V5 and V8 cases on Router A. That's one passing and one refused case per obligation; retries of accepted and refused requests; the view fixture; the same tool call over TCP and over the Unix socket.
- **Reads:**
  - every answer, against spec 1's examples and error table;
  - at named checkpoints, by the store readback route (§2), the §3a tables (PD2): rows exist only in the typed tables, every key is canonical UUIDv7 text, business keys are unique, `project_feed` positions are dense, and `time_basis = router` with a time for every live record.
- **Fails for:**
  - a build that writes M2's `(kind, id)` tables or an integer key;
  - a retry that creates a second record;
  - a view that reads a stored aggregate instead of records.

**OB2: replication to the central copy** (`board-replication`; T-local-tokio, then T-local-workerd).
- **Does:** runs spec 2's V3 list with A holding project P and B reading:
  - a clean write;
  - a transaction larger than one piece;
  - a repeated piece;
  - a gap, refused and then resent;
  - a dropped connection, and a central restart, mid-replication;
  - a schema change that replicates;
  - a too-new store;
  - a project created while central is unreachable, written, then registered; and another one rejected;
  - `cursorSuperseded`;
  - the X3 cases for the feed-to-capture mapping: splits, gaps, lost acknowledgements and snapshots.
- **Reads:** A's views (provisional, replicated, voided); A's `capture_feed_marks`; the copy's records and feed positions through the operations view and B's read copy; the directory summary.
- **Fails for:**
  - a hole or a duplicate after the drop;
  - a reader that sees part of a large transaction;
  - an acknowledgement mapped to the wrong feed position;
  - a summary taken from a stale mark rather than from the captured rows;
  - a rejected project that's deleted rather than moved aside.

**OB3: holder changes** (`holder-change`; T-local-tokio, then T-local-workerd).
- **Does:**
  - a clean move A → B;
  - a move refused with a migration pending, through the real holder. A `failed` state is carried unchanged; that's proven by the library's registry test (consumer-fit VD SO52), because no writer produces it. That missing producer stays a named limit;
  - a move refused `holderUnreachable`, once with A's session closed and once with A's last renewal aged past 1800 s. The age rule runs through the registry's `Clock` capability at 1799 s, 1800 s and 1801 s, at the production value, and one short real-timer case checks the scheduling;
  - a takeover with a preview and a confirm, after A wrote a tail past the cut;
  - RC-4 on both sides of the tail report. Before A's report is admitted, a retry on B of a tail request from the old epoch answers `unresolved{takeover}` and executes nothing. After the report, a retry of a request the tail holds answers `voided`, naming the takeover. A retry of an old-epoch request the tail doesn't hold is admitted fresh. No retry repeats an effect: no new record, feed position, notify intent, read-mark change or unread notice;
  - the report kept for later holders: after B has taken A's report, central restarts and a second takeover moves the project to C. C retrieves the same report and records the fact. A same-digest report replays the stored one, and a different digest answers `tailConflict`;
  - `previewStale`;
  - a takeover that supersedes a move still draining;
  - a cancel;
  - claims on an unheld project from OB5, and B's claim answered `alreadyHeld` naming A. That claim is repeated after a later takeover to B, and it still names A;
  - a claim from a build whose declared support is below the copy's schema;
  - a claim on an archived board;
  - the archive race. The test-only hold at the conformance seam (one-core §13) pauses a claim after the Fleet object has durably reserved it on an unheld project. Archive starts and the board stays `archiving`. Central restarts during the hold. Then:
    - the claim is granted: the directory update and settlement land in one transaction, and the board doesn't finalize while the project is held;
    - or the claim is refused: the reservation settles, and only then does the board finalize to `archived`.
    No reader ever sees `archived` while a reservation is active;
  - a fence-then-crash: central stopped between fence and grant through the test-only fault wrapper (one-core §13), and the same request repeated;
  - X4's successor recovery: after the takeover, B derives the owed notifications from replicated handoffs.
- **Reads:**
  - every answer in board names, phases and outcomes (spec 3 E27);
  - the board's archive state and `claim_reservations` through the operations view, at each step of the archive race;
  - the transfer record; the copy's fence;
  - both Routers' writer state and views;
  - the facts in the current holder's store;
  - A's superseded file, opened read-only, and its tail report.
- **Fails for:**
  - a grant before the copy acknowledges the final position;
  - a move that shows a reservation step;
  - a takeover that leaves two holders;
  - a repeat that issues a second epoch;
  - `alreadyHeld` naming the current holder rather than the recorded one;
  - one global schema maximum;
  - a superseded file reopened for writing;
  - a retry that stays `unresolved` after the report, or is `voided` before it;
  - archive finalization that ignores an active reservation;
  - a tail report deleted after the first holder takes it.

**OB4: across machines** (`cross-machine`, T-two-machines).
- **Does:** runs spec 3's V7 and V10:
  - posts forwarded to an online holder, then to an offline one;
  - fleet requests from machine 2 while central is unreachable, then after it returns;
  - every agent messaging every agent, both ways, as messages and as notifications;
  - local independence, with A's session to central paused: a notification whose cause was written on A goes at once to its local recipient on A. Nothing about it waits on central (PD4 §4b, MS-5a);
  - the replication gate, with only the copy's acknowledgement held back. The test-only hold sits at the conformance seam on central's copy apply, so mail transport stays usable:
    - an unrelated direct message from A reaches machine 2's recipient during the hold, which shows the mail path is open;
    - the remote recipient's notification doesn't arrive during the hold;
    - after release, it arrives once;
  - one cause, two recipients: one cause notifying a local and a remote actor makes two frozen handoffs with distinct item IDs (one recipient each, PD4; spec 1 E18). Each delivery and each retry keeps its own item's ID. MS-5a's takeover exception stays as specified;
  - retention after receipt: once the recipient has the item and it's retired, central's operations view shows the item's ID, digest and final status, with the content gone (PD4 C36);
  - cancels; wakes.
- **Reads:** what each recipient session actually receives; sender statuses; the holder's ledger; central's operations view for retained items.
- **Fails for:**
  - a forward that central stores;
  - a remote notification delivered before its cause and handoff replicated;
  - local delivery that waits on central;
  - a gate that releases a remote notification before the acknowledgement. The held acknowledgement with open mail transport catches it;
  - one item shared by two recipients;
  - central forgetting a delivered item's identity;
  - a second delivery after a resend;
  - an item lost when the recipient's machine is offline.

**OB5: the upgrade and import** (`board-upgrade`, T-legacy-copies).
- **Does:**
  - `--dry-run`, then a real run into central's admin import;
  - interrupted runs: one stopped before the copy is published, and one after publication but before completion, each rerun;
  - a rerun on identical input, and a run with changed content under an existing ID;
  - a store pair with a workspace-name and a board-name collision (X2's fixture);
  - X2's mixed main/reply topic-bookmark case;
  - then a claim.
- **Reads:**
  - the report;
  - the imported workspace, board and directory records as central's fleet feed, a snapshot and a Router's cache return them: each shows its time as unknown. A rerun at another wall-clock time gives identical fleet digests (review F4);
  - each new project database: IDs kept one-to-one, counts, dense positions under the import epoch, keys accepted by the validator, `time_basis = legacy_unknown`, bookmarks and watches as cutoffs, and memberships with their policy, expiry, generation and ending (a draining one ended at the last recorded store time);
  - central's `admin_imports` and copy state;
  - X2's oracle, comparing each reader's covered post set with the shipped inbox selection (`inbox_records.rs:124`).
- **Fails for:**
  - a translated bookmark whose covered post set differs;
  - an imported workspace, board or directory entry carrying an invented time (from its UUID, the run clock or a file time);
  - an invented time;
  - an import reported complete before publication;
  - a second publication on a rerun;
  - a renamed collision;
  - an imported project granted again after a claim.

**OB6: central's runtimes** (`central-runtime`, T-local-tokio and T-local-workerd, plus the conformance harness).
- **Does:** runs the one-core V-OC2 to V-OC5, V-OC7 and V-OC8 scenarios through `central_router` and the object entrypoints on each runtime:
  - commit, rollback after a callback error and after an SQL error;
  - an interrupted handler resumed by identity;
  - a lost HTTP response, then the same request repeated;
  - integer and real extremes;
  - the same HTTP and WebSocket requests to both runtimes;
  - one object held while another answers;
  - the Tokio runtime restarted, with storage, an alarm and a session surviving;
  - a forged caller refused on the wire;
  - a successful grant end to end, which X1 didn't reach.
- **Reads:** responses and statuses; durable state through the operations view and, on Tokio, each object's store by the stop-then-reopen route (§2).
- **Fails for:**
  - an effect before commit;
  - a repeat that applies twice;
  - an integer that crosses as a float;
  - a status that differs between runtimes;
  - an alarm lost on restart;
  - a forged caller accepted.

**OB7: deployed central** (`central-staging`, T-staging).
- **Does:** runs spec 3's V9d and spec 2's V3d:
  - fence and takeover crash cases;
  - a session resumed across a deploy, including a deploy during replication;
  - a real Access assertion verified by signature, while a decoded-only assertion is refused;
  - every other address refused, with no `workers.dev` and no preview URL;
  - the upgrade headers (Access plus machine credential) arriving through the deployed edge, which is X6's open point;
  - a typed upgrade refusal's reason header (W-C13) surviving the deployed edge back to the Router's reqwest client, read from the response the client actually received; an Access-originated 401 or 403 without that header reads as `Unknown`;
  - an idle period long enough for objects to be evicted, then resume;
  - machine credentials refused on admin routes.
- **Reads:** the same as OB2 and OB3, through the deployed edge only.
- **Fails for:**
  - a session that restarts on a deploy;
  - an edge that strips the credential headers;
  - an edge that strips the refusal reason header while the Router reports a typed reason, or a header-less Access refusal given a reason;
  - a decoded-only assertion accepted;
  - state lost to eviction.

**OB8: structural gates** (no journey).
- **Checks:** V-OC1's review that each class of central policy lives only in the core and that agent-fleet-sync's mechanisms are used, never copied; V-OC6's build gates in each owning repository (clippy `-D warnings` under that repository's lint table, `unsafe_code = "forbid"`, and the per-target dependency graph in which the core has no runtime, driver, `worker`, `tokio` or `reqwest`); the typed call DAG's compiler gate, checked in review (U68, U74: no compile-fail tests).
- **Leaves unevaluated:** behavior. OB6 and OB7 observe that.

**OB9: one copy's failure on a shared session** (part of `board-replication`, T-local-tokio).
- **Does:** A holds P1 and P2 on its one session. P2's copy is damaged through a test-only corruption of its central store, then repaired.
- **Reads:** P1's acknowledgements; P2's answers; P2's sender state; P2's piece identity after repair.
- **Fails for:** a closed session; `AnswerLost` for P2; P1 blocked; a new piece identity after repair.
- **Depends on:** agent-fleet-sync SO57 (its `copy-containment` journey).

**OB10: the cross-project merge with unknown times** (no journey; a focused test over the real per-project prefix and sweep harness).
- **Cases** (the Advisor's MergeTime check of `46b9c80c`):
  1. two projects with unknown-time prefixes, then live records, paged in both directions;
  2. a page break at the change from unknown to known time;
  3. a newly included all-unknown project after the sweep, under the new-scope exclusion and fresh-browse rule;
  4. a known wall clock that steps backwards.
- **Reads:** the page contents and the next cursor.
- **Passes when:** one project's ascending order stays `[Unknown@1, Known100@2, Known50@3]`, never position 3 before 2, and no record is skipped across pages.
- **Fails for:** a merge that sorts globally by time.

## 4. What each requirement is proven by (owner's view)

| U | In the owner's words | Proven by | Still unverified |
|---|---|---|---|
| U1 | the environment proves the work | Two Routers run the real role and view cases, and the Lead's view shows the right holder and next obligation (OB1) | — |
| U2 | anyone can learn what's true now | The real views answer from records, and imported history shows its time as unknown, never invented (OB1, OB10, OB5) | — |
| U3, U17 | the renamed structure, end to end | Workspace, board, project and thread work through the API and CLI under the new names, and old IDs carry over in the upgrade (OB1, OB5) | — |
| U4, U5, U7, U8, U12, U13, U18, U40, U43, U44, U64, U65 | tasks, moves, roles, workstreams, ADRs and subjects behave as specified | Each rule's passing and refused cases run through the real API on the new schema (OB1) | — |
| U6, U19, U52 | — | superseded (U40; U57); nothing to prove (§5f) | — |
| U9, U10 | IDs and the feed are fabric-ready | Every stored key is canonical UUIDv7, and retries return the first decision, on one machine and then across two (OB1, OB2) | — |
| U11 | decisions are server-first; facts ship as appends | A takeover keeps the old holder's tail readable and records it as facts. A retry from the old epoch waits as `unresolved` until the tail is reported, then is voided, or admitted fresh if the tail doesn't hold it (OB3) | — |
| U14 | the board hands notices to interactions | A notification reaches its recipient only through the interaction path. A local recipient gets it at once; a remote one gets it only after the cause replicates (OB1, OB4) | — |
| U16, U23 | a project lives on one machine | A write on a machine that doesn't hold the project is refused, and a post is forwarded (OB2, OB4) | — |
| U20 | fleet records are written only by central | Fleet changes from a machine wait as pending while central is down, then land once (OB4) | — |
| U21 | one writer at a time | A clean move, a takeover and a crash between fence and grant never leave two holders (OB3, OB7) | — |
| U22, U32 | Turso on machines; Cloudflare for central | The project file is Turso, and the copy lives in Durable Object storage on staging (OB2, OB7, OB8) | — |
| U24, U46 | central knows what projects exist | Metadata shows each project's status, holder and freshness without reading project tables (OB2, OB3) | search ranking beyond the spec's filters |
| U25, U67 | one database per project, made manageable | Each project has its own file and copy; the operations view lists, migrates and retires them (OB2, OB3) | rebuilding a damaged held copy (G32) |
| U26–U31 | one collaboration API | The same call over TCP and over the socket gives the same answer (OB1) | — |
| U33, U78 | replication is generic and per project | The board's records replicate through agent-fleet-sync with the board's schema plugged in, and nothing in the library knows the board (OB2, OB8) | — |
| U34, U35, U36, U49, U77 | one session per machine that survives its connections | Connections are dropped, central restarts and a deploy happens, and every project resumes from its acknowledged position (OB2, OB7) | a non-WebSocket transport (none exists yet) |
| U37, U38 | leases fence writers; 4 hours, renewed every 15 minutes | Expiry pauses writes, wakes and schedules, and renewal resumes them on the same epoch (OB2, OB3) | — |
| U39, U45 | holder changes are explicit admin requests | Moves, takeovers and their refusals run through the admin routes, including the 30-minute unreachable bound (OB3, OB7) | — |
| U41, U48 | decisions by level; ADR text in the workspace record | ADRs and the decision register behave as specified, and central shows them read-only (OB1, OB2) | — |
| U42, U47 | the work trail | Rows are insert-only except redaction, and replicate (OB1, OB2) | — |
| U50, U53 | central is tested locally and on Cloudflare | The same scenarios pass on the Tokio runtime, local workerd and staging (OB6, OB7) | — |
| U51 | owner words are relayed | Relayed owner words show "relayed by", never as signed (OB1) | first-hand capture (G3) |
| U54, U60, U62 | all history kept through a one-time upgrade, by ID | Real store copies upgrade with IDs, order, bookmarks and memberships kept and nothing invented; collisions are reported and refused; reruns are safe (OB5) | owner collision policy (G28) |
| U55, U57, U58 | not on the open web; admin is the owner's sign-in | Only Access-verified requests reach central, and admin needs the owner's sign-in (OB7) | — |
| U56, U61 | holding and messaging are separate | A post into a project held elsewhere is forwarded and answered (OB4) | — |
| U59 | public and private repositories split | Release review checks the public repository holds placeholders only | not a behavior of this design; no observation here |
| U63, U66 | messages and notifications reach every agent | Every agent messages every agent across machines. Remote notifications wait for replication while local ones don't, and wakes stay bounded (OB4) | human recipients (G30) |
| U68 | the typed call DAG | The compiler gate in review, plus a forged caller refused on the wire (OB8, OB6) | — |
| U69–U75 | one async core on two real runtimes | The core runs the same scenarios on Tokio and Cloudflare with the structural gates green (OB6, OB7, OB8) | — |
| U76 | exact integers | Integer extremes cross both runtimes unchanged (OB6) | — |
| U79 | the new board schema | Every stored board record sits in §3a's typed tables with UUIDv7 keys, through live writes and the upgrade (OB1, OB5) | — |

## 5. Coverage rows

Columns: U · R (and spec V) · seam (trace row) · observation · target, journey and lifetime · passes when, and the wrong build that fails · reuses · unverified.

**Reuse, by boundary.**
- **Pure rules PD1 still owns:** M2's tests in `crates/message-board/tests/` at `1e0b5529` (for example `move_rules_contracts.rs` and `domain_contracts.rs`) have no SQL dependency, and they're reused, with the `RecordTime` fixture adapted where a record carries a time.
- **Integration tests:** M2's and M3's integration tests are judged by what they actually drive. Those written against M2's schema or central's own engines decide nothing here, because this revision replaces both.
- **Old runs:** they're never new evidence. Fixtures and oracles can be reused separately from passing results.

The rewritten product tests come from the plan, not from this document.

### 5a. Board records (spec 1, PD1, new schema)

| U | R (V) | Seam | Obs. | Target · journey · lifetime | Passes when · wrong build | Reuses | Unverified |
|---|---|---|---|---|---|---|---|
| U1 | RL-4, VW-3 (V1, V5) | PD1 §11 RL-4, VW-3 | OB1 | T-local-tokio · `board-records` · lasting | the Lead's view names the holder and next obligation spec 1 gives for the fixture · a view that reads a stale cached assignment | none | — |
| U2 | MV-3, RC-5, RC-9, RC-12, VW-1…VW-8 (V2, V5, V6) | PD1 §11; PD2 §9 spec 1 §5 row | OB1, OB5, OB10 | T-local-tokio · `board-records` · lasting; the OB10 test | live records carry the accepting Router's time; imported ones show unknown, and a time filter never matches them; the merge keeps each project's order · a build that fills an imported time from the UUID or the run time | none | — |
| U3, U17 | ST-1, API-6 (V1, V4, V8) | PD1 §11 ST-1, API-6 | OB1, OB5 | T-local-tokio · `board-records` · lasting; T-legacy-copies · `board-upgrade` · temporary | every renamed tool and CLI name answers, and the old project, board and topic IDs become workspace, board and project IDs · an old name still accepted, or an ID minted anew | none | — |
| U4, U5, U7, U8, U12, U13, U18, U40, U43, U44, U64, U65 | ST-1…ST-6, MV-1, MV-2, MV-4, MV-7, MV-8, RL-1…RL-8, WS-1…WS-3, TK-1…TK-7, TH-1…TH-4, VW-2, VW-5 (V1, V5) | PD1 §11's rows for each | OB1 | T-local-tokio · `board-records` · lasting; pure rules (`move_rules`, `work_rules`, `request_canonical`) by focused tests, with no app launch | each obligation's passing case gives spec 1's answer and stored rows. Each refused case gives the named failure with the subject's business state unchanged, plus exactly the durable outcome its obligation specifies: an admitted `move_request` refusal records the rejected move, its feed entry, its ledger decision and the lead's notify intent once (MV-3, RC-4, RC-12), and a retry adds nothing. `move/check` and refusals before scope writes nothing · a cycle accepted in `blockedBy`; a second lead granted; a status change without a move; a refused move that drops its record or notifies twice | M2's pure-rule tests in `crates/message-board/tests/` (`1e0b5529`), for the rules only, with the `RecordTime` fixture adapted | — |
| U9, U10 | RC-1…RC-8, RC-10, RC-11, RL-2, RL-3, WA-7 (V2, V3) | PD1 §11 RC rows; PD2 §9 WA-7 | OB1, OB2 | T-local-tokio · `board-records`, `board-replication` · lasting | every stored key is canonical UUIDv7 text, and retries of an accepted post, an accepted move and a refused move each return the first decision · an integer or v4 key; a retry that writes again | none | — |
| U11 | RC-4, RC-10, RC-11, WA-5 (V2, V3) | PD1 §11 RC-4; PD2 §9 WA-5 | OB3 | T-local-tokio · `holder-change` · lasting | before the tail report, an old-epoch retry answers `unresolved{takeover}` and executes nothing; after it, a held ID answers `voided` naming the takeover and an absent one is admitted fresh; no retry repeats an effect · a build that stays `unresolved` forever, voids before the report, refuses an absent ID, or replays `accepted` for a voided one | none | — |
| U14 | MV-3, NI-1…NI-4, RL-3, RC-12, VW-8 (V1, V5) | PD1 §11 NI rows; PD4 §11 | OB1, OB4 | T-local-tokio and T-two-machines · `board-records`, `cross-machine` · lasting | intents and handoffs are written with their cause; the recipient gets the notification only through the interaction path; a local recipient isn't held by the copy's acknowledgement, and a remote one is · a board that pushes directly; local delivery blocked on central | none | — |
| U26–U31 | API-1…API-8 (V8) | PD1 §11 API rows | OB1 | T-local-tokio · `board-records` · lasting | the same tool call over TCP and over the Unix socket gives the same status and body · a listener that answers differently | none | — |
| U41, U48 | ST-1, ST-5, WT-3; spec 3 WA-1 (V1) | PD1 §11; one-core §13 WA-1 | OB1, OB2 | T-local-tokio · `board-records` · lasting | an ADR's text never changes after creation, and the workspace record carries it · an edit accepted | none | — |
| U42, U47 | RC-5, VW-1, WT-1…WT-6, API-6 (V1, V2, V5, V6, V8) | PD1 §11 WT rows | OB1, OB2 | T-local-tokio · `board-records`, `board-replication` · lasting | rows are insert-only apart from `redacted`, and B's read copy shows the same rows · an update to row text accepted | none | — |
| U51 | spec 1 §6 identity and security (G3 A) | PD1 §11 RC-8 | OB1 | T-local-tokio · `board-records` · lasting | a relayed owner word shows "relayed by" its agent · a view that shows it as the owner's own | none | first-hand capture (G3 B, C) |
| U79 | U79's binding rule over every spec 1 entity (PD2 §3, §3a) | PD2 §3 binding table | OB1, OB5 | T-local-tokio · `board-records` · lasting; T-legacy-copies · `board-upgrade` · temporary | every board record is in its §3a table with a canonical UUIDv7 key, and none in an M2 table, through live writes and the upgrade · an M2 table still written | none | — |

### 5b. Project storage and replication (spec 2, PD2)

| U | R (V) | Seam | Obs. | Target · journey · lifetime | Passes when · wrong build | Reuses | Unverified |
|---|---|---|---|---|---|---|---|
| U16, U23 | WA-2 (V3) | PD2 §9 WA-2 | OB2, OB4 | T-local-tokio, T-two-machines · `board-replication`, `cross-machine` · lasting | a non-post write on B for A's project is refused `notWriter{current}`, and a post is forwarded · a write accepted on B | none | — |
| U21, U33 | WA-3, WA-3a, WA-3b, WA-6, WA-7 (V3, V3d) | PD2 §9 | OB2, OB7 | T-local-tokio, T-local-workerd · `board-replication` · lasting; T-staging · `central-staging` · lasting | provisional becomes replicated at the mapped position, a large transaction appears whole, a repeat changes nothing, a gap is refused `positionGap` then resent, and a superseded writer is refused · a reader seeing half a transaction; a duplicate after resend | agent-fleet-sync's own suites for its mechanisms (its SV rows), at the pinned tag; they don't use the board's schema or bookkeeping | — |
| U25 | WA-8 (V3) | PD2 §9 WA-8 | OB2 | T-local-tokio · `board-replication` · lasting | each project has its own file and exactly one copy · two projects in one database | none | — |
| U22, U32 | WA-9 (V3) | PD2 §9 WA-9 | OB2, OB7, OB8 | T-local-tokio, T-staging · lasting | the holder's file is Turso; the copy is Durable Object storage on staging; no libSQL or `sqld` in any graph (OB8) · a libSQL dependency | the drivers' own suites | — |
| U20, U40 (WA-10) | WA-10 (V3) | PD2 §9 WA-10 | OB2 | T-local-tokio · `board-replication` · lasting | a project created while central is down is written, then replicates once accepted; a rejected one answers `projectRejected` and its file is moved aside, not deleted · a deleted file; a write lost at registration | none | — |
| U37, U38 | WA-11, FL-9 (V3, V9) | PD2 §9 WA-11; one-core §13 FL-9 | OB2, OB3 | T-local-tokio · `board-replication`, `holder-change` · lasting | after expiry, writes answer `leaseExpired{expired_at}` and wakes and schedules pause, and renewal resumes them on the same epoch. The production values are consumed, not just configured: a lease issued under the default `BoardLeasePolicy` reads back `expires_at` as grant plus 14,400 s, and renewal is scheduled 900 s apart, checked through the `Clock` capability at those boundaries. One short configured lease exercises the real timer and pause path · a pause that also blocks reads; a default that's configured but unused | none | four hours of elapsed wall time: the controlled clock stands in for it, and the short real lease checks the timer integration |
| U34, U35, U36, U49, U77 | WA-12, FL-11, FL-12 (V3, V3d, V9, V9d) | PD2 §9 WA-12; one-core §13 FL-11, FL-12 | OB2, OB7 | T-local-tokio · `board-replication` · lasting; T-staging · `central-staging` · lasting | after a dropped connection, a central restart and a deploy, the same session resumes, every project continues from its acknowledged position, and nothing is lost or repeated · a new session per connection; a reapplied piece | none | a non-WebSocket transport (U77 is structural until one exists: the session uses only `SessionTransport`, OB8) |
| U63 (WA-14) | WA-14 (V10) | PD2 §9 WA-14; PD4 §11 | OB4 | T-two-machines · `cross-machine` · lasting | see U63, U66 below | none | as U63 |
| U78, U79 (spec 2 §4.1) | §4.1's realizations (V3, V3d) | PD2 §9 U78 row | OB2, OB8 | as WA rows | as WA rows, plus OB8: no agent-fleet-sync crate depends on a board crate · a library crate that imports `message-board` | none | — |

### 5c. Central and the fleet (spec 3, one-core PD)

| U | R (V) | Seam | Obs. | Target · journey · lifetime | Passes when · wrong build | Reuses | Unverified |
|---|---|---|---|---|---|---|---|
| U20, U41 | WA-1 (V7, V9) | one-core §13 WA-1 | OB4 | T-two-machines · `cross-machine` · lasting | machine 2's fleet request waits `pendingUnreachable` while central is down, then lands once with its original request ID · a Router writing fleet records locally | none | — |
| U24, U46 | FL-1, FL-2 (V9) | one-core §13 FL-1, FL-2 | OB2, OB3 | T-local-tokio · `board-replication` · lasting | metadata shows name, workspace, board, status, holder or `unheld`, lease state and each part's freshness, and search filters by them, without central reading project tables · a summary that lags the replicated feed row, or central writing a copy | none | ranking quality |
| U21, U25, U37 | FL-3, FL-4 (V9, V9d) | one-core §13 FL-3, FL-4 | OB2, OB3, OB7 | T-local-tokio, T-staging · lasting | a piece from an older epoch is refused `staleEpoch` whole, a new epoch takes effect only between pieces, and a full copy answers `centralCopyFull` · a fence that lands mid-piece | none | a real full Durable Object (injected by the test-only wrapper, not by filling 10 GB) |
| U3 | FL-5 (V9) | one-core §13 FL-5 | OB3 | T-local-tokio · `holder-change` · lasting | a board reads `archiving` until no project accepts work writes, then `archived`; reader-state writes still replicate; a claim mid-archive is refused `archived`; an already reserved claim keeps the board `archiving` across a restart until it's granted or refused and settled · `archived` shown early; finalization that ignores `claim_reservations` | none | — |
| U9 | FL-6, FL-7 (V9) | one-core §13 FL-6, FL-7 | OB2 | T-local-tokio · `board-replication` · lasting | missed notices are caught up from a cursor; a cursor older than seven days answers `cursorExpired`, checked through the `Clock` capability just before and after the seven-day boundary at the production value; one short configured retention exercises the real purge; a piece whose answer is lost stays unknown until the acknowledged position is read · a correctness decision that depends on a notice; a seven-day default that's never consumed | none | seven days of elapsed wall time (the controlled clock stands in) |
| U11 | FL-8 (V9) | one-core §13 FL-8 | OB3 | T-local-tokio · `holder-change` · lasting | the old holder's report reaches the current holder, which records `RejectedTail`; after a restart and a second holder change, the next holder retrieves the same report; a same-digest report replays it and another digest answers `tailConflict` · holders talking directly; a report deleted after its first uptake | none | — |
| U39, U45 | FL-10 (V7, V9, V9d) | one-core §13 FL-10 | OB3, OB7 | T-local-tokio, T-local-workerd · `holder-change` · lasting; T-staging · `central-staging` · lasting | each OB3 case answers as spec 3 FL-10 states, with the phases per kind, the takeover superseding a draining move (owner decision), `holderUnreachable` naming last-seen and last-renewed separately, and a repeat after a crash finishing on the same epoch · OB3's wrong builds | agent-fleet-sync's holder-change suites at the pinned tag, for the library's state machine only | — |
| U55, U57, U58 | FL-13 (V9d) | one-core §13 FL-13 | OB7 | T-staging · `central-staging` · lasting | only Access-verified requests reach any route, no other address answers, a machine needs both its service token and its credential, and admin routes need the owner's sign-in · a decoded-only assertion accepted; `workers.dev` reachable | none | — |
| U56, U61 | FL-14 (V7, V9) | one-core §13 FL-14 | OB4 | T-two-machines · `cross-machine` · lasting | a post for a project held elsewhere is forwarded with the sender's verified installation and answered; offline holder → `holderUnreachable`; unheld → `notHeld`; no answer in 30 s → `unanswered`; more than 16 in flight → `overloaded`; central keeps no post content · a forward stored at central | none | — |
| U54, U57 | FL-15, FL-16, FL-17 (V4, V9) | one-core §13 FL-15…FL-17 | OB5, OB3 | T-legacy-copies · `board-upgrade` · temporary; T-local-tokio · `holder-change` · lasting | an imported project is unheld and refuses writes `notHeld`; a claim is granted the next epoch and seeds before admitting; identical re-import answers `alreadyImported` even after the claim (SO55); an interrupted import finishes under the same identity on both sides of publication (G35) · an import marked complete before publication; a second grant after a claim | the consumer-fit VD's SO55 row | — |
| U67 | FL-18 (V9, V9d) | one-core §13 FL-18 | OB3, OB7 | T-local-tokio · `holder-change` · lasting; T-staging · lasting | retirement happens only when every precondition holds; afterwards every piece, import, claim and read answers `retired`, ahead of replays; a repeat returns the first outcome · a retired ID reused | none | re-seeding a damaged held copy (G32) |
| U63, U66 | MS-1…MS-12, WA-14 (V10) | PD4 §11; one-core §13 MS row | OB4 | T-two-machines · `cross-machine` · lasting | each MS row's case as PD4 §11 and spec 3 V10 state, observed at the recipient session, including the held-acknowledgement case and identity retention after receipt (OB4) · PD4's wrong builds; local delivery blocked on central; a forgotten item identity | agent-fleet-sync's delivery suites at the pinned tag, for custody only | human recipients (G30); questions and approvals (MS-12, deferred) |
| U78, U79 (spec 3 §4.2) | §4.2's mapping; OC-20 (V9, V9d, V10) | one-core §13 OC-20 | OB3, OB4, OB7 | as above | every board surface answers in board names while the wire uses agent-fleet-sync's; the one behavior change is FL-10's · a library name leaking to a board answer | none | — |

### 5d. Central's one core (one-core spec)

| U | R (V) | Seam | Obs. | Target · journey · lifetime | Passes when · wrong build | Reuses | Unverified |
|---|---|---|---|---|---|---|---|
| U69 | OC-1…OC-6 (V-OC1, V-OC3, V-OC7, V-OC9) | one-core §13 | OB6, OB8 | T-local-tokio, T-local-workerd · `central-runtime` · lasting | OB6's step, interruption and overlap cases, plus a real Router over the real session (V-OC9): disconnect before send, a commit whose answer is lost, replay, catch-up and Router restart · a runtime that holds policy; a step that effects before commit | none | — |
| U70, U22 | OC-7, OC-8 (V-OC6) | one-core §13 | OB8 | structural | each driver exists once, in agent-fleet-sync, pinned by tag · a driver crate in this workspace | — | — |
| U32, U75 | OC-16, OC-17, OC-18 (V-OC2, V-OC8) | one-core §13 | OB6, OB7 | T-local-tokio, T-local-workerd, T-staging · `central-runtime`, `central-staging` · lasting | the same scenarios pass on each runtime, and the Tokio runtime keeps storage, alarms and sessions across a restart · a Tokio-only fallback in the Router | none | — |
| U68 | OC-21 | one-core §13 OC-21 | OB8, OB6 | structural; T-local-tokio · `central-runtime` · lasting | the compiler gate holds in review, and a forged caller is refused `CallEdgeRefused` on the wire · a receiver that trusts the envelope | — | — |
| U76, U33 | OC-10 (V-OC4) | one-core §13 OC-10 | OB6 | T-local-tokio, T-local-workerd · `central-runtime` · lasting | i64 MIN and MAX, 2^53−1, 2^53, 2^53+1, REAL 1.0, NULL, text and blob cross both runtimes unchanged, including a rowid beyond 2^53 · an integer read as a JS number | none | — |
| U71, U30 | OC-12, OC-13 (V-OC5, V-OC6) | one-core §13 | OB6, OB8 | as OB6 | the same HTTP and WebSocket requests give the same statuses and bodies on both runtimes · a route that exists on one runtime only | none | — |
| U72, U73 | OC-9, OC-11, OC-14, OC-15 (V-OC4, V-OC6) | one-core §13 | OB8, OB6 | structural; OB6 for stored-value refusal | each owning repository's gate passes under its own lint table, with no `unsafe`; a corrupt stored value is refused `Corrupt{field}` · a relaxed lint table | — | — |
| U74 | OC-19 | one-core §13 OC-19 | how every row above is carried out | — | no row relies on a source-scan or compile-fail test | — | — |
| U50, U53 | OC-18 (V-OC2) | one-core §13 OC-18 | OB6, OB7 | as U32, U75 | one harness, run on both runtimes, with a separate Verifier observing | none | — |

### 5e. The upgrade (spec 1 §5, PD2 §3d)

| U | R (V) | Seam | Obs. | Target · journey · lifetime | Passes when · wrong build | Reuses | Unverified |
|---|---|---|---|---|---|---|---|
| U54, U60, U62 | spec 1 §5 (V4); FL-17 (V4) | PD2 §9 spec 1 §5 row | OB5 | T-legacy-copies · `board-upgrade` · temporary | IDs are kept one-to-one; per-project order and density hold; bookmarks and watches cover the same post sets as the shipped selection, including the mixed main/reply case; memberships keep their policy, and draining ones end at the last recorded store time; imported times are unknown; collisions are reported and the real run refused; a rerun answers `alreadyImported`; changed content answers `importConflict` · OB5's wrong builds | X2's oracle and fixtures (unpublished working notes), extended; they ran on SQLite through Python, so they decide the derivation rule, not the production binary | the owner's real stores beyond the copies provided; the collision policy (G28) |

### 5f. Requirements with nothing to observe here

| U | Why | Where it's carried |
|---|---|---|
| U6, U19 | superseded by U40: milestones are retired (spec 1 E4; ST-2, ST-4 retired) | spec 1 coverage |
| U52 | superseded by U57: admin is the owner's own sign-in | U55, U57, U58's row in §5c |
| U59 | a repository policy, not a behavior of this design: no spec obligation names it | release review checks that the public repository holds placeholders only. That's a named limit: this design adds no observation for it |

## 6. Limits the owner should know

None of these waives a required observation.
- **Waiting on owner decisions, not on proof:** G28 (what a name collision becomes), G30 (where a human recipient's messages land) and G5 (fact retention). Each row that touches one names it.
- **Library-dependent rows** (OB3's claim refusals, OB9, U54's re-import) can't pass until agent-fleet-sync's W-C lands at a tag. Until then they're unproven, not failed.
- **Controlled clock and short bounds:** the production values (14,400 s, 900 s, 1800 s, seven days) are proven consumed through the `Clock` capability at their boundaries. Short configured values exercise the real timers. Elapsed wall time at the production lengths isn't observed.
- **Store readback** happens through the owning process or by stop-then-reopen (§2). A concurrent external read path isn't qualified.
- **Injected faults:** a full copy, a crash between fence and grant, and a damaged copy are injected through test-only wrappers around the real capability (one-core §13). Real disk-full and real I/O faults stay unobserved.
- **Hosted behavior** is observed only on T-staging, and only after the `central-staging` journey exists.
