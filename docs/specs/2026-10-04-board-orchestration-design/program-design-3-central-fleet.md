# Program design 3: the central fleet

> **0.1.102 cut (owner decision 1217, 2026-10-24).** Process-definition registration and its fleet-admission rules were removed with project moves. Everything else stands.

**What this is:** how central realizes [spec 3](spec-3-central-fleet.md). **Its runtime structure is superseded (2026-10-16):** C20, §2a, the `storage_transaction` component, the crate list and the `collaboration-central-local` adapter are replaced by [the central one-core program design](../2026-10-16-central-one-core/program-design.md). That design describes one async core with capabilities injected per runtime; see [what went wrong](../2026-10-16-central-one-core/what-went-wrong.md). The domain design here (copies, fences, leases, transfers, sessions, fleet, forwarding, imports, operations) still governs. Central is a Rust Cloudflare Worker (workers-rs) with four kinds of Durable Object:
- one connection object per machine;
- one central copy per project;
- one lease registry;
- one fleet object.

The Worker's edge also has a separate admin endpoint. The holder side of replication (capture, pieces, the session client, lease states) is in [program-design-2-project-storage.md](program-design-2-project-storage.md).


**Revised 2026-10-17 for agent-fleet-sync (U78, U79).** Central now composes agent-fleet-sync's engines instead of building its own (spec 3 §4.2; [the one-core program design](../2026-10-16-central-one-core/program-design.md) §0, which governs). This document stays as the record of central's board rules. Each section's status:

| Section | Status | Where it's governed now |
|---|---|---|
| §1 C7 (Cloudflare Worker and Durable Objects), C17 (one registry object), C10 (the fleet feed) | holds | one-core §0, §0a |
| §1 C8, C9 (atomic apply; a fence only between pieces) | realized by agent-fleet-sync's receiver over `DurableObjectStore` | agent-fleet-sync SO1–SO3, SO16 |
| §2 object layout: a connection object per installation, a copy object per project, one registry, one fleet object, the admin operations | holds. Each object hosts agent-fleet-sync's rules for its face | one-core §0, §4 |
| §2 the edge's two-layer authentication | holds, in `CentralApplication` | one-core §4, §5 |
| §2 `storage_transaction` and the crate list; §2a | superseded (2026-10-16) | one-core |
| §3 bindings and shapes | superseded | one-core §3 |
| §4 applying a piece | superseded by agent-fleet-sync's receiver. The board name `centralCopyFull` is mapped | agent-fleet-sync SO2, SO3, SO9; spec 3 §4.2 |
| §5 takeover and move | the mechanism is superseded by agent-fleet-sync's holder-change workflow (SO15, SO16, SO48, SO50, SO52), and the schema check by SO53's per-target projection. Still central's: the admin routes, the preview presentation, presence for `holderUnreachable`, `claim_reservations` (step 2a), the fleet import and the admin-import completion | spec 3 FL-10, FL-16, FL-17; one-core §0, §0a, §13 |
| §6 other paths: the rules | hold as board rules: creation and directory acceptance, fleet requests, the fleet feed and notices (one notice stream per installation, in its connection object), rejected-tail reports (retained and retrievable by every later holder), archive and metadata. Replicated reads use the library | one-core §0, §13 |
| §6 other paths: physical storage | superseded where it names keys or tables. `fleet_feed` is keyed by a UUIDv7 `entry_id` with `position` unique; `_session_notices` becomes the connection object's `machine_notices`; `tail_reports` is keyed by a UUIDv7 `report_id` with `(project_id, epoch)` unique | one-core §0a |
| §7 failure, recovery and concurrency | piece, fence and transfer recovery is agent-fleet-sync's durable operations (SO31, SO42). Central's own flows recover from their own rows | one-core §8 |
| §8 proof seams; §9 trace | superseded | one-core §13; the board and central Verification Design |
| §10 open items and constraints, including the call DAG (U68) and the hosted call-depth budget | hold | one-core §5 (K9); spec 3 |
| §11 operating the copies | the operations view and migration sweep hold as central's. Copy migration history and retirement are agent-fleet-sync's (SO47), and the `Retired` sums map to its retired answers | one-core §0, §13 |

```text
requirements.md ──► spec 3 ──► program-design-3-central-fleet.md (this) ──► plan
```

Evidence:
- the platform facts for this rewrite (unpublished working notes), with Cloudflare limits as of 2026-10-09: 10 GB per object on Paid, 2 MB per row, 32 MiB per received WebSocket message, 30 s of CPU per event (configurable to 5 min), deploys disconnect every WebSocket;
- the workerd-local atomicity tests (2026-10-08 and 2026-10-11). The official workers-rs `Storage::transaction()` makes several SQL writes all-or-nothing: it commits on success and rolls back on a SQL error, a Rust `Err` or a fence mismatch, even across an `.await`. It blocks every other event to the object until it finishes, as the matching workerd source shows (`blockConcurrencyWhile`). Writes made outside a transaction aren't rolled back;
- the Advisor's lease memo (unpublished working notes).

Results from workerd-local runs are labelled local until a deployed check confirms them (spec 2 G18).

## 1. The choices

| Crux | Options | Selected | Why | Reopen if |
|---|---|---|---|---|
| **C7. Platform and process shape** | (a) a self-hosted axum server with a Turso hub (the old design); (b) one Cloudflare Worker with Durable Objects | **(b)** (owner, U32, U34) | The Turso hub's push isn't atomic and its server is dev/test only. Durable Objects give one single-threaded actor per project with private transactional storage | — |
| **C8. Applying a piece atomically** | (a) the official workers-rs `Storage::transaction()`, which binds Cloudflare's `ctx.storage.transaction()`; (b) `transactionSync`, reached through `as_raw()` and reflection because workers-rs doesn't wrap it; (c) a TypeScript shim | **(a)** (owner, 2026-10-11: use supported APIs only) | Cloudflare forbids `BEGIN` and `SAVEPOINT` through `exec`, and a multi-statement `exec` isn't atomic. Cloudflare documents that on SQLite-backed objects any `sql.exec()` inside `transaction()` is part of the transaction. Tested locally with workers-rs 0.8.7 and workerd 2026-10-06: it commits on success, rolls back on a SQL error, a Rust `Err` and a fence mismatch, and rolls back across an `.await`. Another request to the same object ran only after the transaction finished. (b) works too, but it's unsupported, so it isn't used | the deployed check (G18) disagrees with the local result |
| **C9. Keeping a fence from landing mid-apply** | (a) an in-memory per-project gate (the old gateway); (b) the central copy is the only place both apply and fence run, each as one synchronous transaction with no `await` inside | **(b)** | An object runs one event at a time, and a synchronous transaction can't be interleaved. A fence can only land between pieces (FL-3) with no lock to hold or lose | — |
| **C17. Where the lease registry lives** | (a) one global registry object; (b) registry shards; (c) D1 as the authority; (d) inside each project's copy (option A) | **(a)** | The owner chose a separate lease structure (U37), which rules out (d). (a) serializes every lease and transfer with no cross-shard protocol. At 15-minute renewals, a fleet of dozens of machines is a few requests a minute. D1's zero-row update is still a success, so it can't fence on its own | measured renewals and transfers approach one object's limit (a soft 1,000 requests/s) |
| **C10. Routers' copy of fleet records** | (a) Routers pull a whole fleet database (the old design); (b) the fleet object streams a fleet feed by cursor over the session | **(b)** | There's no hub to pull from. A cursor-based feed gives Routers a local fleet cache that keeps working while central is unreachable (WA-1) | — |
| **C11. Notices** | (a) server-sent events (the old design); (b) frames on the machine's session WebSocket | **(b)** | The session already exists for replication, and a hibernated WebSocket costs nothing while idle | — |
| **C18. The admin credential** | (a) a separate bearer token, its hash a Worker secret, held by the owner; (b) Cloudflare Access with the owner's sign-in | **(b)**, owner 2026-10-13 (U57) | The network is single-user. Moves and takeovers are rare and always done by the owner, so a short-lived sign-in beats a stored secret. Google is already the org's identity provider | more admins than the owner, or unattended admin automation |
| **C23. Keeping central off the open web (U55)** | (a) Cloudflare Access in front of a dedicated hostname; (b) central on a tailnet machine; (c) WARP-enrolled devices | **(a)**, owner 2026-10-13 | A Worker can't join a tailnet, and a self-hosted central is only as available as its machine, while leases renew every 15 minutes and cross-machine posts route through central. WARP is ruled out for now. Verified on the owner's account: an Access service token gates a WebSocket upgrade to a Workers Custom Domain (401 without, 101 with), and the client's `Authorization` header reaches the Worker unchanged (experiment 2026-10-13) | WARP becomes acceptable (add a device check), or the owner wants no public DNS name |
| **C19. Detail reads of a project from another machine** | (a) central runs board views; (b) a read-only snapshot pulled from the central copy, labelled with its position | **(b)** | It reuses seeding (PD2) in read-only mode, so central stays schema-agnostic: it never needs board view logic. Every page carries the copy's acknowledged position. If the position moves between the first and last page while the holder is writing, the reader starts again, so a snapshot is always one consistent position. Seeding a new holder needs no restart: once the copy is fenced it refuses the old holder's pieces, and the new holder seeds before it admits anything, so nothing applies to the copy while it seeds | projects grow large enough that snapshots are slow, or a busy project keeps restarting them; then add incremental catch-up |
| **C20. Where central's logic lives** *(superseded 2026-10-16 by the one-core program design: "thin adapters" became two runtime applications)* | (a) inside the Cloudflare objects; (b) a platform-independent core crate with storage and messaging ports, and thin platform adapters injected through one interface (§2a): Cloudflare for production, a native local server for testing | **(b)** (owner, 2026-10-11) | The rules must not be tied to Cloudflare: whether to apply or refuse a piece, how the fence moves, lease renewal and expiry, the transfer steps, and fleet admission. In a core crate they compile for native and wasm32, can be unit-tested natively against an in-memory SQLite stand-in for the storage port, and could run on another host later. Only storage, sockets, hibernation, alarms, credentials and the transaction call are Cloudflare-specific | — |
| **C21. Wall-clock times on the wire** (review F9) | (a) `DateTime<Utc>` serialized as RFC 3339; (b) epoch milliseconds as `u64` | **(a)** | It's what `collaboration-protocol` already sends (`push_record.rs`, `created_at`), and the Rust rules require timezone-aware UTC. Lease expiry, last-seen and acknowledgement times use it; elapsed amounts stay explicit durations (`remaining_ms`). The registry's clock is the only clock that decides expiry (FL-9). Checked 2026-10-11 on toolchain 1.98.1: `chrono` 0.4.45 with `default-features = false, features = ["serde", "alloc"]` builds for `wasm32-unknown-unknown`, and an RFC 3339 `DateTime<Utc>` round-trips through serde_json. The protocol crate never reads the clock, so it needs no `clock` feature | — |
| **C22. Board identity in `ArchiveSeen`** (review F9) | (a) a `String`; (b) a `BoardId` UUIDv7 newtype in `collaboration-replication-protocol`, the same identity as `message-board`'s `BoardId` (`board_identity.rs`), converted at the holder's boundary | **(b)** | Board IDs are UUIDv7 (U10) and get the same validation as the protocol's other IDs. The protocol crate stays free of board-crate dependencies, since it builds for wasm32 with serde, serde_json and thiserror only | — |

**Costs:**
- Central now depends on Cloudflare's availability. The 4-hour lease absorbs outages of up to about 4 hours with no loss.
- Rust on workers-rs means wasm32 constraints.
- Project data lives in Cloudflare storage (U32).

## 2. Components

```mermaid
flowchart TB
  R1["Router (holder of A)"] -- "WebSocket /v1/session + machine credential" --> EDGE
  R2["Router (holder of B)"] -- "WebSocket /v1/session" --> EDGE
  OWNER["owner CLI"] -- "HTTPS /admin/v1/* + owner sign-in (Access)" --> EDGE
  subgraph W["Cloudflare Worker (Rust, workers-rs)"]
    EDGE["edge: authenticate, then route\n(machine credential ≠ admin credential)"]
    ADMIN["admin_operations"]
  end
  EDGE -- "machine sessions" --> CO1["ConnectionObject (one per installation)"]
  EDGE -- "machine sessions" --> CO2["ConnectionObject"]
  EDGE -- "admin requests only" --> ADMIN
  CO1 -- "pieces, snapshot reads" --> PCA[("ProjectCopyObject A")]
  CO2 -- "pieces" --> PCB[("ProjectCopyObject B")]
  CO1 & CO2 -- "renew, release, forward route, reserve first epoch" --> LR[("LeaseRegistryObject (one)")]
  CO1 <-- "forwarded post, its answer (correlated, §2a)" --> CO2
  CO1 & CO2 -- "fleet requests, feed, tail reports" --> FO[("FleetObject (one)")]
  ADMIN -- "move, takeover, enroll" --> LR
  LR -- "fence, initialize" --> PCA & PCB
  LR -- "notices" --> CO1 & CO2
  LR & PCA & PCB -- "directory updates" --> FO
  FO -- "feed, notices" --> CO1 & CO2
```

| Component | Job | Reason to change | Consumers | Owns |
|---|---|---|---|---|
| `edge` (Worker fetch handler) | authenticate each request in two layers, then route.<br>**Layer 1, the Access gate:** verify `Cf-Access-Jwt-Assertion` (RS256 with the team's JWKS, cached and re-fetched on an unknown `kid`; `iss` is the team domain; `aud` is the AUD of the Access app that owns the path; `exp`, and `nbf` when present: Access service-token assertions carry no `nbf`, which staging found on 2026-10-15).<br>**Layer 2, by route:** `/v1/session` needs a service-token assertion (`type: app`) whose `common_name` is the Access client ID that the `LeaseRegistryObject` recorded for an enrolled installation, **and** that same installation's machine credential in `Authorization`. If they name different installations, the request is refused. It forwards the WebSocket upgrade to that installation's `ConnectionObject`. `/admin/v1/*` needs the admin app's assertion naming the owner's identity and goes to `admin_operations`.<br>A service-token assertion never works on the admin route, and a sign-in assertion never works on the session route | the Access team domain, the two AUD tags and the owner identity, as Worker configuration from the private ops repo and never in the public repo. The client-ID-to-installation map is enrollment state in the registry, so adding a machine needs no redeploy | Routers, the owner's CLI | nothing stored; it never forwards a credential to an object, only a verified caller identity and class |
| `ConnectionObject` | one per installation. It accepts the session WebSocket with the hibernation API and keeps the socket's `{installation, session_id, connection_id}` as an attachment. It decodes each `MachineFrame` (PD2 §3) and dispatches it to the copy, registry or fleet object (a `Claim` goes to the registry, FL-16), then answers with the same `frame_id`. It answers `Hello` with `Welcome` (each project's acknowledged position and lease). It pushes notices to the socket. A newer connection's `Hello` for the same installation takes over the logical session and closes the older socket with `Closed{ConnectionReplaced}`; requests in flight on the older socket get no answer and are re-sent by the machine (FL-12, U49). A `Hello` with a different session ID replaces the session record; acknowledged positions live in the copies, so nothing is lost. It keeps the credential's expiry from the upgrade and refuses requests after it with `Closed{CredentialRejected(Expired)}`, with no registry call per request; a revocation closes the installation's session through the registry (`Closed{CredentialRejected(Revoked)}`) | session protocol | Routers | the session record (session ID, installation, credential expiry) and the forwarding correlation rows `pending_forwards` and `delivered_forwards` (§2a), in its own storage |
| `ProjectCopyObject` | one per project, addressed by project ID. It applies pieces (§4), fences and initializes, serves snapshot pages for seeding and read copies, and stages the parts of large transactions | replication format | `ConnectionObject`, `LeaseRegistryObject`, `admin_operations` (import) | the replicated tables and `_copy_state` (fence epoch, start position, acknowledged position, initialized, archived), `_copy_staged_parts`, `_copy_transfers` (fence receipts by transfer ID) |
| `LeaseRegistryObject` | the only issuer of leases (E23). It renews, reserves first epochs, runs moves and takeovers as recorded transfers, keeps the epoch floor, and enrolls and revokes machines. It also records imported projects as `Unheld{import_epoch, imported_head, schema_version}` (FL-15) and runs claims (FL-16) as recorded transfers of kind `Claim`, fencing the copy before it grants (§5 Claim) | lease rules | `ConnectionObject`, `admin_operations` | `leases`, `transfers`, `epoch_floor`, `machines` (installation, credential hash, enrolled or revoked), `last_seen` |
| `FleetObject` | fleet admission (WA-1), the project directory and metadata (FL-1), the fleet feed and notice outbox (FL-6), rejected-tail reports (FL-8) and board archive (FL-5) | fleet entity rules | `ConnectionObject`; `LeaseRegistryObject` and `ProjectCopyObject` (directory updates); `admin_operations` (fleet import) | fleet records, the fleet request ledger, the feed, the outbox, tail reports, archive state |
| `admin_operations` | move, takeover (preview, then confirm), machine enrollment, and **admin import** (FL-17). `/admin/v1/import/fleet` installs fleet records with their original IDs through fleet admission's import path. `/admin/v1/import/projects/{id}` streams a project's snapshot pages, one `ProjectCopyObject::ImportSeed{project, digest, index, page, final_summary}` call per page (the last page carries the summary), which initializes an unheld copy at the imported head, then records it with the registry (§5 Import). Both are keyed by ID and digest: identical content answers `AlreadyImported`, whether or not the project has been claimed since; different content answers `ImportConflict{existing, offered}`, and a held or once-claimed copy is never touched. It only calls registry, fleet and copy methods marked admin | admin rules | `edge` | nothing stored |
| `storage_transaction` | the Cloudflare transaction shell: one function that runs one core `step` with a `SqlTx` inside the official `Storage::transaction()`. The core's synchronous work closure runs inside it with no `.await`. Every write in every object goes through it | workers-rs API | all objects | — |

**Logic and platform:** every component in the table above is a thin Cloudflare shell over `collaboration-central-core` (C20). The shell owns transport and storage, and the core owns every decision.

**Dependency direction** (ranks and the compile-time rule in §10): `edge` → (`ConnectionObject`, `admin_operations`). `ConnectionObject`'s session face → (copy, registry, fleet, another installation's delivery face for a forwarded post, §2a). `admin_operations` → (registry, copy import, fleet import). Registry → copy (fence, initialize), fleet (lease directory updates) and delivery (notices). Copy → fleet (directory updates). Fleet → delivery (feed and notices). Fleet calls neither the registry nor a copy: creation is two sequential session calls, session → fleet and then session → registry (§5 Creation). No object calls `edge`, and nothing but `admin_operations` reaches a registry admin method. Objects aren't publicly addressable: only the Worker's own code holds their stubs. Each object checks the caller class the edge stamped on the internal request.

**Crates** (C20). *(Superseded 2026-10-16. The crate list and the "shares decisions, not storage" split below produced two runtime applications and a third copy of fleet admission in the Router; [the central one-core program design](../2026-10-16-central-one-core/program-design.md) replaces them.)* The core holds the rules; the Worker only adapts them to Cloudflare:
- `collaboration-replication-protocol` (native and wasm32): the wire shapes `MachineFrame`, `CentralFrame`, `Piece`, `ReplicationPosition`, `LeaseStatus` (PD2 §3).
- `fleet-admission` (native and wasm32): the fleet admission **rules**, pure, with no IO and no async (M2.3 finding: one storage port can't be both the Router's async `sqlx-turso` and central's synchronous `SqlTx`). It shares decisions, not storage:
  - `reads_for(&FleetRequest) -> FleetReads` is the closed list of keys the request needs: its ledger entry, the subject records and revisions it names, name-uniqueness keys, and process definitions by `name@version`;
  - `decide(&FleetRequest, &FleetSnapshot, accepted_at, ids) -> FleetDecision { decision: RequestDecision, writes: Vec<FleetWrite>, feed: Vec<FleetFeedEntry> }` decides from the snapshot of those keys.
  - Each side runs one transaction: read the keys, call `decide`, then apply the writes, the feed entries and the ledger entry. The Router's in-process stand-in does this on its fleet store through `sqlx-turso`, asynchronously, under `BEGIN IMMEDIATE`. Central's `FleetObject` does it through `SqlTx` inside its core step.
  - The same decision tables run against both adapters: M2 for the stand-in, M3.5 for central.
- **`collaboration-central-core`** (native and wasm32, no Cloudflare dependency): all of central's decisions.
  - **Copy rules:** given a piece and the copy's state, decide `Refuse(reason) | AlreadyApplied(acked) | StagePart | Apply(plan)`, and turn an apply plan into ordered SQL with parameters (upsert and delete by primary key, schema steps).
  - **Fence rules.**
  - **The lease registry state machine:** renew, expiry against a supplied clock, the epoch floor, one active transfer per project.
  - **The move and takeover steps:** fence, then grant; the repeat-safe transfer.
  - **Session frame dispatch.**
  - **One injected interface, `SqlTx`.** Everything else is returned as effects, or passed in as arguments (§2a).
  - The core's own unit tests need no IO: they call the decide functions with a state, a time and IDs, and check the new state and the effects. The conformance suite runs the same scenarios through both platforms (§2a).
- **`collaboration-central-cloudflare`** (wasm32 only, the Cloudflare adapter): it implements `SqlTx` over Durable Object SQL, inside the official `Storage::transaction()` it opens (`storage_transaction`). It also owns the Worker's routes, credential checks, the hibernation WebSocket handling, socket attachments, alarms and stub calls between objects. Each object is a thin shell: load its state, call the core, write through its storage port. It's built with `worker-build`, outside the native workspace build, with its own CI job. Its lints deny `unwrap`, `expect` and `panic!`: a panic hangs the request and leaves writes made outside a transaction (local test). The core's lints deny them too.

## 2a. One interface, two platforms (strict)

> **Superseded (2026-10-16): don't build from this section.** It realized central as a *synchronous* core that received only storage. Each runtime got its own application around it: a Cloudflare one and a native one on Turso's engine, each with its own routes, Access check, effect loop and transaction sequence. That violated the owner's requirements: one async core injected into every runtime through a common trait (requirements U69), with SQLx as the database API (U70). The 2026-10-11 owner words quoted below asked for one interface and injection. "Only storage is injected", "no async" and a second native application were the Lead's own inferences, never the owner's. Replaced by [the central one-core program design](../2026-10-16-central-one-core/program-design.md); the full account is in [what went wrong](../2026-10-16-central-one-core/what-went-wrong.md). Kept as history.

Central's rules live in one platform-free crate. **Only storage is injected**, because it's the one thing that genuinely differs between platforms. Anything else that touches the outside world is returned as data for the platform to carry out. Time and IDs are passed in as plain arguments. (Owner, 2026-10-11: "a common interface and inject", "very strict", "run it locally during testing", "not everything should be injected", "no unsafe, same standards".)

```mermaid
flowchart TB
  subgraph CORE["collaboration-central-core (pure: no IO, no async, no platform crates)"]
    DECIDE["decide(input, state, now, ids) → (new state, Vec&lt;Effect&gt;)\ncopy apply · fence · lease rules · transfer steps · fleet admission · frame dispatch"]
    SQL["SqlTx: the only injected interface\nsynchronous execute / query of core-built statements,\ninside a transaction the shell opened"]
    CONF["conformance suite: the same scenarios for every platform"]
  end
  CF["collaboration-central-cloudflare (wasm32)\nopens the official async Storage::transaction(); inside it, SqlTx = Durable Object SQL\ncarries out effects: stub calls, hibernated WebSockets, alarms"] -- implements --> SQL
  LOCAL["collaboration-central-local (native)\nopens a Turso transaction on the actor's blocking thread; SqlTx = that transaction\ncarries out effects: actor messages, axum WebSockets"] -- implements --> SQL
  CF -. runs .-> CONF
  LOCAL -. runs .-> CONF
```

**The one interface (review F6: one contract, sync on the core side):**
- The core sees only `trait SqlTx { fn execute(&mut self, statement: BoundStatement) -> Result<u64, StoreError>; fn query(&mut self, statement: BoundStatement) -> Result<Vec<SqlRow>, StoreError>; }`. It's synchronous, and it's always an open transaction the shell started.
- `BoundStatement` has a constructor private to the core: the core builds every statement and maps the rows they return into typed records.
- **Exact numeric transport (M3.6 finding).** workers-rs binds and returns SQL numbers as JavaScript doubles (worker 0.8.7 `sql.rs`), which loses integers beyond 2^53 and can't tell `REAL 1.0` from `INTEGER 1`. So the core encodes every value it binds, and every column it reads, in a form both engines carry exactly.
  - Integers are bound as decimal text inside `CAST(? AS INTEGER)`; reals as numbers inside `CAST(? AS REAL)`; text and blobs as they are.
  - Reads select `typeof(col)`, with integers read as `CAST(col AS TEXT)`, so a value's storage class and its full i64 value are never inferred from a double.
  - The shells only bind the parameter kinds the core declares (`BoundValue::{IntegerText, Real, Text, Blob, Null}`) and return rows of those kinds; they never convert numbers themselves.
  - The shared numeric conformance scenario (minimum and maximum i64, the adjacent integers above 2^53, REAL 1.0) must pass on Turso and under workerd.
- The core's entry point is synchronous: `fn step(tx: &mut dyn SqlTx, input: CoreInput, now: RegistryTime, ids: IdBatch) -> Result<Vec<Effect>, CentralError>`. An `Err` makes the shell roll the transaction back.
- **The shell owns the transaction and everything async.**
  - Cloudflare: the object calls the official async `Storage::transaction()`. Inside its callback it wraps the object's SQL storage as a `SqlTx` and calls `step` once, with no `.await` between the first and last statement. This is the shape the 2026-10-08 probe ran: a synchronous body inside the official transaction, rolled back on `Err`.
  - Local: the actor runs `step` on its blocking thread, inside a Turso transaction it opened there. **Evidence (2026-10-12 experiment):** `turso_core` 0.8.1, with default features, opens a file and runs BEGIN, statements, then COMMIT or ROLLBACK synchronously, with no runtime: `Database::open(io, path, OpenOptions)`, `connect()`, `prepare()`, and a `step()` loop that drives `IO::step()`. The same function works inside `tokio::task::spawn_blocking` under one Tokio runtime. **Caveat:** a failed statement doesn't roll back its transaction by itself, so on any `Err` from `step` the local shell issues `ROLLBACK` explicitly before answering. `Busy` and `Interrupt` are errors. Unverified: contention, deferred IO, crash durability, and equivalence to Durable Object storage; the conformance suite covers behavior, not durability.
  - After commit, the shell carries out the effects and feeds each result back as the next `CoreInput`.
- The local shell's engine is not the holder's store path. Holders use Turso through `sqlx-turso` (U22). Central's local shell emulates Durable Object SQLite, which is synchronous, so it uses Turso's engine directly on a blocking thread, with one Tokio runtime and no nested runtime.

**Effects are returned, not injected.** A core step returns `Vec<Effect>`, with closed variants:
- `Send { to: ConnectionId, frame: CentralFrame }`: `Welcome` after a `Hello`, `Answer{frame_id, …}` after a request, `Notice`, or `Closed{reason}`, after which the shell closes that socket. The core decides every frame and its recipient; the shell only resolves the ID to a socket and writes it (review F6; M3.3 finding).
  - `ConnectionId` is a UUIDv7 the shell mints for each accepted WebSocket. It's passed in with `CoreInput::Hello` and every `CoreInput::Frame`, kept in the socket attachment `{installation, session_id, connection_id}`, and stored in the session record as the session's current connection.
  - On a newer `Hello`, the core sends `Closed{ConnectionReplaced}` to the previous `ConnectionId` and `Welcome` to the new one, and records the new one as current (U49).
  - An `Answer` goes only to the connection its request came on. If that connection has since been replaced, the answer is dropped: the machine re-sends on the new connection, and re-sending is safe by identity (FL-12).
- `Notify { installation, notice }`;
- `Dispatch { call_id, to: ObjectAddress, call: ObjectCall, carry: Option<DispatchCarry> }` (the carry is a transient, core-owned value the shell returns with the call's result and never persists; `DispatchCarry::{ForwardPost{post}, ForwardDelivery{holder_last_seen}}`), where `ObjectAddress::{Copy(ProjectId), Registry, Fleet, Connection(InstallationId)}` and `ObjectCall::{ApplyPiece{piece, sender: InstallationId}, Fence{transfer_id, new_epoch, holder: InstallationId, expect, target_schema_max}, Initialize{project, epoch, holder: InstallationId}, CopyStatus{project}, SnapshotPage{cursor}, Renew{attempt, project, epoch}, Release{…}, ReserveFirstEpoch{project}, TailReport{…}, Fleet(FleetRequestEnvelope), Read{scope, after}, ArchiveSeen{…}, DirectoryUpdate(DirectoryUpdate), ForwardRoute{project, sender: InstallationId}, DeliverForward{forward_id, origin, origin_label, project, post}, ForwardAnswered{forward_id, project, outcome: ForwardOutcome}, Claim{attempt, project, claimant: InstallationId}, ImportSeed{project, digest, index, page: SnapshotPage, final_summary: Option<ProjectSummary>}, RecordUnheld{project, import_epoch, imported_head, schema_version}}`. `ForwardRoute`, `DeliverForward` and `ForwardAnswered` are forwarded posts (below); `Claim`, `ImportSeed` and `RecordUnheld` are claims and imports (§5). Frame dispatch is a core decision: the connection object's core turns each frame into one `Dispatch`, and the shell only makes the stub call;
- `Return { call_id, result: ObjectCallResult }`: the target object's typed answer to an incoming `CoreInput::Call`. Every incoming call gets exactly one `Return` over its whole handling, which may span several steps (M3.3 finding). A call whose answer depends on another object, such as a registry `Release` that has to fence the copy before it can grant, returns `Dispatch` and no `Return` from its first step. It persists its continuation, for example the transfer in `Fencing`. The shell keeps driving that inbound call: it carries out the dispatch, feeds the `CallResult` back as the next input, and steps again until the call's `Return` appears. Each step is its own transaction. The conformance suite checks exactly one `Return` per inbound call across its handling: none at the end, or two, is an error. A crash mid-handling leaves the persisted continuation, and the caller's repeat of the same request (same identity) resumes it. The shell routes it back to the caller after commit. On Cloudflare it's the stub call's response; locally it's the actor's reply channel. The caller's core receives it as `CoreInput::CallResult` (implementation finding, M3.1);
- `SetAlarm { at }`.
The result of a `Dispatch` comes back as `CoreInput::CallResult { call_id, result: ObjectCallResult, carry: Option<DispatchCarry> }`: `result` is a closed enum with one variant per `ObjectCall`, and `carry` is the same transient value the `Dispatch` carried, handed back from the shell's working memory and never persisted (review F23). It's `None` for every call that wasn't dispatched with one. The core turns it into the `Send` of the matching `Answer`. Inputs are `CoreInput::{Frame{frame_id, request}, Hello{…}, CallResult{call_id, result, carry}, CallFailed{call_id, carry}, Call{from: ObjectAddress, call_id, call} (an incoming dispatch on the target object), Alarm, Admin(AdminRequest)}`. `CallFailed` reports a dispatch the target never answered (a stub error or unreachable object). Today it's fed only for forwarding dispatches, which turn it into `Forwarded(Unanswered)` and free the reserved slot (M3.8 finding). Other dispatch failures keep their existing handling. `from` is set by the calling shell; only this Worker's own objects make calls, so it's trusted.

**Forwarded posts (spec 3 FL-14).** Every decision is in the core; central keeps no post content and no outcome, only a short-lived correlation.
1. **Sender's `ConnectionObject`, `Frame{Forward}`.** In one transaction it counts this installation's `pending_forwards` rows, in any phase. At 16 it answers `Forwarded(Overloaded)`. Otherwise it **reserves the slot** by inserting `pending_forwards(forward_id, frame_id, connection_id, project, phase: Routing, deadline = now + 30 s)`, sets an alarm for the deadline, and dispatches `Registry: ForwardRoute{project, sender}`. Because the check and the reservation commit together, two forwards can't both take the last slot while their route lookups are pending.
2. **`LeaseRegistryObject`** answers `ForwardRouteResult::{Route{holder, sender_label, holder_last_seen}, SenderHolds, NotHeld}`. `sender_label` is the enrolled installation's machine label, the authoritative source for `origin_label`. `holder_last_seen` is the registry's last-seen time for the holder, the same source holder-change refusals use (FL-10).
   - **The post isn't stored while the route is looked up.** Step 1's `Dispatch` carries it as `carry: Some(DispatchCarry::ForwardPost{post})`. The shell holds that value in memory only for the duration of that call and hands it back as `CoreInput::CallResult{call_id, result, carry}`. A carry is never persisted. If the object restarts mid-call, the carry and the call are lost together, and step 7's deadline answers `Unanswered`.
3. **Sender, on the route.** It acts only if the matching `pending_forwards` row still exists in phase `Routing` and its deadline hasn't passed. A missing, expired or already-advanced row means the forward was already answered `Unanswered`, so the late result produces no answer, no delivery and no new slot.
   - `SenderHolds` or `NotHeld` is answered at once, and the reserved row is deleted. So is a failed registry call, which is answered `Unanswered`.
   - On `Route`, it updates the row to `phase: AwaitingHolder{expected_holder: holder}`, keeping the original deadline, and dispatches `Connection(holder): DeliverForward{…}`.
   - When `DeliverForward`'s own result comes back, `HolderUnreachable` is answered only if the row still exists in `AwaitingHolder` within its deadline. Otherwise it's ignored.
4. **Holder's `ConnectionObject`, `Call{DeliverForward}`.**
   - With no current connection, it returns `NoConnection`. The sender answers `HolderUnreachable{last_seen: holder_last_seen}` and deletes its row. Step 3's `DeliverForward` dispatch carries `DispatchCarry::ForwardDelivery{holder_last_seen}`, so the value survives that call without being stored; the row still holds only IDs, connections, phase and deadline.
   - Otherwise it inserts `delivered_forwards(forward_id, origin, project, deadline)`, sends `Forwarded{…}` to its current connection, and returns `Delivered`.
5. **Holder, `Frame{ForwardAnswer}` from its own machine.** It's accepted only for a `forward_id` in `delivered_forwards`; any other answer is dropped. The holder deletes the row and dispatches `Connection(origin): ForwardAnswered{forward_id, project, outcome: Answered{reply}}`.
6. **Sender, `Call{ForwardAnswered}`.** It's accepted only when `pending_forwards` holds that `forward_id` for that `project` in phase `AwaitingHolder{expected_holder}` and the call's `from` is `Connection(expected_holder)`.
   - The sender deletes the row, sends `Answer{frame_id, Forwarded(Answered)}` to the stored `connection_id`, and returns `Ack`.
   - If that connection has been replaced, the answer is dropped and the machine re-sends with the same `request_id`; the holder's ledger replays it.
   - A late, unknown or wrong-sender answer gets `Ack` with no send.
7. **Alarm.** An expired `pending_forwards` row, in either phase, becomes `Answer{Forwarded(Unanswered)}` on its stored connection and is deleted, which frees its slot. An expired `delivered_forwards` row is deleted.

A holder change during a forward needs no special case: the old holder's admission refuses the post (`NotWriter`), and that answer reaches the sender as `Answered`. The rows hold IDs, connections and deadlines only, never a post or a reply.

The platform shell carries them out after the transaction commits, and feeds any result back as the next input. A takeover is the stored transfer record moving through explicit steps (`Fencing` → `Granted`), not one async function. A crash between steps leaves a record the next step can resume.

**Time and IDs are arguments.** For example, `renew(lease, now: RegistryTime)` and `begin_takeover(request, transfer_id: TransferId, now)`. The shell reads the clock and makes the IDs; tests pass fixed values.

**Standards: the same as the rest of the workspace, with no exceptions for wasm.**
- `#![forbid(unsafe_code)]` in the core and in both platform crates. Only supported workers-rs APIs are used; there's no `as_raw()` or reflection.
- No `unwrap`, `expect`, `panic!`, `todo!` or `unimplemented!` outside tests, under the workspace lints.
- Closed enums with payloads for every state and result, newtype IDs (`ProjectId`, `TransferId`, `InstallationId`, `Epoch`), `thiserror` domain errors, Serde-tagged wire enums. Untrusted frames are validated once at the edge.
- Files under 1,000 lines; formatting and clippy with `-D warnings` on both targets.

| Rule | Enforced by |
|---|---|
| The core has no platform crates: no `worker`, `wasm-bindgen`, `js-sys`, `web-sys`, `tokio`, `axum`, `reqwest`, `sqlx`, `rusqlite`, and no Turso engine crate (`turso`, `turso_core` or any other `turso_*`). The only exceptions, allowed by exact name, are `turso_parser` and its compile-time `turso_macros`: pure parsing with no IO, needed for the schema fingerprint (PD2), and verified to build for wasm32 | a CI check on `cargo tree -p collaboration-central-core -e normal` |
| The core has no async, no `cfg(target_arch)` and no global state | a CI grep; the core builds for native and wasm32 with no platform features |
| Every decision is in the core; a shell only loads, calls, commits and carries out effects | the conformance suite runs on both platforms, so a shell that decides differently fails it |
| Replicated changes reach storage only as core-built statements | the private constructor |

**The two platforms:**
- **`collaboration-central-cloudflare`** (production): `SqlTx` is Durable Object SQL inside the official `Storage::transaction()`, which the shell opens. It carries out effects through stub calls between objects, hibernated WebSockets and alarms, and owns the Worker's routes and credential checks.
- **`collaboration-central-local`** (testing and development): a native axum server (U30) with the same `/v1/session` WebSocket and `/admin/v1/*` routes.
  - Each object is one actor, a Tokio task with one event at a time, as in a Durable Object.
  - Its `SqlTx` is a Turso transaction on the actor's blocking thread, one file per object, with explicit `ROLLBACK` on error (verified 2026-10-12; see §2a).
  - Routers and the debug Host point at it with one setting, so the whole system runs on one machine with no Cloudflare account.
  - It's what V3 and V9 run against natively. It isn't a production target unless the owner chooses that later.

## 3. Bindings and shapes

| E | Semantic owner | Home | Schema / type home | Shape at each boundary | Kept as |
|---|---|---|---|---|---|
| E23 lease | `LeaseRegistryObject` | new | registry storage `leases(project, holder, epoch, expires_at)`; wire `LeaseStatus` in `collaboration-replication-protocol` | renew → `Renewed{expires_at, transfer_in_progress, replayed, current}` (PD2 §3) or `Refused(StaleEpoch{current_epoch, epoch_ended_at} \| NotWriter{current})`; renewal attempts are kept by attempt ID so a re-send replays its recorded result; admin → `MoveResult = Moved{new_epoch, start} \| Pending{transfer_id} \| Refused{holderUnreachable \| migrationPending \| transferInProgress}`; a move completes asynchronously, so the admin's call answers `Pending` until the holder has released, and repeating the same request (same `transfer_id`) answers the final outcome``; `TakeoverPreview{holder, epoch, acked, last_seen, tail_estimate, confirm_token}` (the token is bound to holder, epoch and acked, §5); `TakeoverResult = Granted{new_epoch, start} \| Pending{transfer_id} \| Refused{previewStale \| schemaTooNew \| transferInProgress}`; `MoveResult` also refuses `schemaTooNew`; the registry keeps `machines(installation, credential hash, enrolled or revoked, schema_support, last_seen)` | persisted |
| E24 central copy | `ProjectCopyObject` | new | `_copy_state` (fence epoch, holder installation (none while unheld), start, acked, schema identity, initialized, archived, and `imported{digest, import_epoch}` for an imported copy), `_copy_receipts` and `_copy_part_receipts` (piece and part receipts, §4), `_copy_staged_parts`, `_copy_transfers` (fence receipts with each old epoch's cutoff and holder), `_copy_summary` (the projection for the directory, §6) in the object's storage | `Acked{through}`, `PartStaged{…}`, `Refused{staleEpoch{current_epoch, epoch_ended_at} \| positionGap{acked} \| pieceConflict \| notInitialized \| centralCopyFull}`; `SnapshotPage{project, position, schema, schema_steps, table, rows, next}` or `ProjectCopyRead::Moved{position}` (PD2 §3) | persisted |
| E25 replication piece | `ProjectCopyObject` (apply) | — | `collaboration-replication-protocol` (PD2 §3), with `digest` and `TransactionPart{transaction, index, count, transaction_digest}` | in from the session; stored only as staged parts and receipts | staged parts; receipts kept permanently (§4) |
| E26 machine session | `ConnectionObject` | new | session row in its storage; socket attachment `{installation, session_id, connection_id}` (well under the 16 KB limit) | `MachineFrame`, `CentralFrame` (PD2 §3) | persisted |
| E1, E2, E7, E16 fleet records | `FleetObject` | new store for existing entities | `fleet-admission` port; tables in the fleet object's storage. E7 holds its immutable Markdown text in the record (U48) | fleet requests and feed entries, as in PD1 | persisted |
| E3 directory entry and metadata (FL-1, U46) | `FleetObject` | new | `directory(project, name, workspace, board, created_by)` written by fleet admission; `directory_status(project, status, latest_record_at, position, acked_at)` from the copy side; `directory_lease(project, lease, as_of, lease_version, digest)` from the registry side, behind one `lease_incarnation_fence` (§6 Metadata), where `DirectoryLease::{Held{holder, state: LeaseState{Held, Expired}, expires_at}, Unheld}` (an unheld project has no holder or expiry; FL-1, FL-15) | `DirectoryQuery{text, workspace, board, status, holder}` → `DirectoryEntry{project, name, workspace, board, board_archive, copy_side: DirectorySide<CopyMetadata{status, latest_record_at, as_of: {position, acked_at}}>, lease_side: DirectorySide<LeaseMetadata{lease: DirectoryLease, as_of}>}`, with `DirectorySide<T>::{Known(T), NotYetReported}` (M3.5 finding). A project created or imported before its first summary or lease update is still listed (FL-1), with `NotYetReported` on the missing side: no status or `Unheld` is made up for it. An imported project's lease side becomes `Known(Unheld)` once its `RecordUnheld` lands. A `status` filter never matches a `NotYetReported` copy side, both inside `FleetPayload` (§6) | persisted |
| E22 rejected-tail report | `FleetObject` | new | `tail_reports(project, epoch, request_ids, digest)` | `TailReport` frame in; `TailReported` notice out | persisted |
| E27 holder change | `LeaseRegistryObject` | new | `transfers(transfer_id, project, kind, requester, reason, old_epoch, new_epoch, to_installation, state, start, request_hash)`; `transfer_id` is the request ID the admin's client mints (UUIDv7), so a repeat after a lost answer names the same transfer; the same ID with a different `request_hash` is `conflictingRequest` | admin results above | persisted |

## 4. Applying a piece (FL-3, FL-4, FL-7, WA-3a/3b)

```mermaid
sequenceDiagram
  participant S as ConnectionObject
  participant P as ProjectCopyObject
  S->>P: apply(Piece)
  P->>P: transaction (official API; synchronous work) {
  alt not initialized
    P-->>S: Refused{notInitialized}
  else piece.epoch < fence, and sender = that epoch's holder (from its fence receipt)
    P-->>S: Refused{staleEpoch, current_epoch, epoch_ended_at (from the fence receipt)}
  else sender ≠ the holder of piece.epoch, or piece.epoch > fence
    P-->>S: Refused{notWriter, current epoch}
  else digest doesn't match the piece's content
    P-->>S: Refused{pieceConflict}
  else a part whose E25 identity (epoch, continues_from, through, index) has a part receipt
    P-->>S: same piece digest → Acked{its transaction's through}; different → Refused{pieceConflict}
  else a whole piece whose identity has a receipt
    P-->>S: same digest → the receipt's recorded answer; different digest → Refused{pieceConflict}
  else piece.through ≤ acked, and no receipt
    P-->>S: Refused{positionGap, acked} (resend from acked; never acknowledged blind)
  else piece.continues_from ≠ acked
    P-->>S: Refused{positionGap, acked}
  else a part of a larger transaction, not the last
    P->>P: stage it under (transaction key, index) with its piece digest; a staged index with a different piece digest → pieceConflict; check count and transaction digest against parts already staged
    P-->>S: PartStaged{transaction, index, acked} (acked unchanged)
  else the last part
    P->>P: assemble the staged parts plus this incoming part (each index once); require indices 0..count−1 all present, same count and transaction digest, and the digest recomputed over the ordered assembly equal to it
    alt any part missing or inconsistent
      P->>P: drop that transaction's staged parts
      P-->>S: Refused{positionGap, acked} (the sender restarts the whole transaction from part 0)
    else complete
      P->>P: apply the validated assembly once, in order; acked = through; move every part's (continues_from, through, index, piece digest) from staging into _copy_part_receipts; clear the staging
      P-->>S: Acked{through}
    end
  else whole transactions
    P->>P: for each transaction in order: SchemaStep → run its statements; Rows → upsert / delete by primary key
    P->>P: acked = through; record the receipt
    P-->>S: Acked{through}
  end
  P->>P: } (any error → the whole transaction rolls back → Refused with the mapped reason)
```

- **Schema-step admission (M3.4 finding: a schema step could otherwise forge central's bookkeeping).** Before running a `SchemaStep`, the core parses each statement with the pinned `turso_parser` (the same one the fingerprint uses) and admits it only if both conditions hold.
  - **Its form is allowed:** `CREATE TABLE`, `CREATE INDEX`, `CREATE VIEW`, `ALTER TABLE`, `DROP TABLE`, `DROP INDEX` or `DROP VIEW`, or `INSERT`, `UPDATE` or `DELETE` for data migrations.
  - **It names no reserved object anywhere** (targets, `FROM`, joins, subqueries, CTEs), as decided by the shared `reserved_namespace` classifier (PD2 §5). The same classifier filters central's snapshot and read enumeration and the row path.
  - Triggers, virtual tables, `ATTACH`, `DETACH`, `PRAGMA`, `VACUUM`, `REINDEX`, `ANALYZE` and transaction control are refused.
  - A refused statement refuses the whole piece as `schemaUnsupported{form}`, a session refusal kind. Nothing applies, and the holder stops that project's replication and reports it.
  - The row path's reserved-table check is also ASCII case-insensitive.
- **Receipts are checked against the fence state.** A fence receipt is trusted only if it agrees with `_copy_state`: its new epoch is the current fence, and its holder is the current holder. Otherwise the fence or release fails closed as an integrity error, the transfer stays where it was, and nothing is granted.
- **Only the holder writes (FL-2; M3.3 finding).** An epoch isn't a credential. The copy records the holder installation together with its fence, and each fence receipt keeps the holder of the epoch it ended, so the copy knows every epoch's holder. The registry sets both through `Initialize` and `Fence`. The epoch is checked first: a superseded holder sending a piece for its own old epoch gets `staleEpoch` with that epoch's cutoff (review F3), while any sender that wasn't the holder of the piece's epoch gets `notWriter`. Every `ApplyPiece` carries the sender's installation, which the connection object takes from the authenticated session, never from the frame. A piece from any other installation is refused `notWriter`. This needs no registry call per piece, so the write path stays off the registry.
- **No `await` inside a transaction's work.** Fetches, notices and logging happen after it returns. That is also why a fence can only land between pieces.
- **A storage-limit error** inside the transaction rolls back, and the copy answers `centralCopyFull` (FL-4).
- **Receipts (review F4).** Each applied piece leaves a receipt, `_copy_receipts(epoch, continues_from, through, digest)`, written in the same transaction as the apply. A re-sent piece is matched on its identity, `(epoch, continues_from, through)`: the same digest gets the acknowledgement it got then (`Acked{through}`, which the identity determines), not the current `acked`; a different digest gets `pieceConflict`. Parts of a larger transaction share one `(continues_from, through)`, and E25's identity includes the part's place, so a part's identity is `(epoch, continues_from, through, index)`. The transaction key `(epoch, commit_id)` names the same `through` (review F4, F20). When an assembly commits, each part's piece digest moves from staging into `_copy_part_receipts(epoch, continues_from, through, index, digest)`, keyed by exactly that identity. A repeated part is checked against its own receipt: unchanged gets `Acked{through}` for its transaction, changed gets `pieceConflict`. A part never meets another part's digest, and a whole piece (no part) uses `_copy_receipts` keyed by `(epoch, continues_from, through)`. Receipts of both kinds are never deleted, because E25 and WA-3b set no replay window. A receipt is about 80 bytes, so even years of pieces stay far below the object's limit. `positionGap` is reserved for a piece that truly doesn't continue from `acked`.
- **Staging (review F2).** A part is staged durably under its transaction key, and `PartStaged` only means "held, not replicated". The replication position moves only when the last part completes a whole, consistent transaction, so no reader ever sees part of one (WA-3a). Cleanup is never a correctness step: the alarm clears staged parts older than a day, or older than the fence's epoch, and a final part that then finds parts missing is refused, never applied short. Each staged part is stored with its own piece digest. A part repeated while staged replaces nothing: the same transaction key and index with the same **piece** digest is a no-op `PartStaged`; a different piece digest is `pieceConflict`, even if it declares the same whole-transaction digest (review F2). The whole-transaction digest is checked only when the assembly is validated. The holder keeps every captured change until it's acknowledged (WA-3b), so it can always restart the transaction.
- **Rows are applied by declared primary key** (PD2 C12): `INSERT … ON CONFLICT(key) DO UPDATE` for upserts and `DELETE … WHERE key = ?` for deletes. Hidden rowids at central are its own and never compared with the holder's. Column order comes from the copy's own schema, which the same migrations built (PD2 C13). Foreign-key checks are deferred to the end of the transaction: rows arrive in the holder's commit order, so constraints hold at commit.
- **The largest transaction** that one event can apply within the CPU limit is unmeasured (G17). A part that keeps failing the same way is reported by the Router as `pieceApplyFailed` and stops that project's replication, rather than retrying forever.

## 5. Takeover and move (FL-9, FL-10)

```mermaid
sequenceDiagram
  participant O as owner CLI
  participant E as edge (admin credential)
  participant L as LeaseRegistryObject
  participant P as ProjectCopyObject
  participant N as ConnectionObjects (old, new holder)
  O->>E: POST /admin/v1/projects/{id}/takeover/preview
  E->>L: preview(project)
  L->>P: status()
  P-->>L: fence epoch, acked, schema identity
  L-->>O: TakeoverPreview{holder, epoch, acked, last_seen, tail_estimate, confirm_token}
  O->>E: POST .../takeover {transfer_id, to_installation, reason, confirm_token}
  E->>L: takeover(...)
  L->>L: one active transfer per project; token's holder and epoch == current lease, else previewStale;<br/>target's last schema_support known, else schemaTooNew;<br/>new_epoch = max(epoch_floor, fence) + 1; transfer = Fencing
  L->>P: fence(transfer_id, new_epoch, expect{epoch, acked}, target_schema_max)
  P->>P: transaction (official API; synchronous work) {<br/>receipt for transfer_id → return it;<br/>fence epoch ≠ expect.epoch or acked ≠ expect.acked → PreviewStale{acked};<br/>schema version > target_schema_max → SchemaTooNew;<br/>else fence = new_epoch; start = acked; record receipt }
  P-->>L: Fenced{start} | PreviewStale{acked} | SchemaTooNew{store, supported}
  L->>L: Fenced → lease = {to_installation, new_epoch, now + 4 h}; epoch_floor = new_epoch; transfer = Granted<br/>Unavailable → transfer unchanged, phase kept, answer Unavailable (§11)<br/>otherwise → transfer = Refused(reason); lease unchanged
  L-->>O: Granted{new_epoch, start} | Refused{previewStale | schemaTooNew | transferInProgress}
  L->>N: on grant: notices LeaseSuperseded to the old holder, LeaseGranted to the new one
```

- **Preview binds the loss boundary (review F1).** The confirm token is bound to the holder, epoch and acknowledged position the preview showed. The registry checks holder and epoch, which it owns. The copy checks epoch and position inside the same transaction that would move the fence, so a piece that commits between confirmation and fencing makes the fence refuse (`PreviewStale`), and nothing changes: no fence, no grant. The admin sees the new position and decides again.
- **Schema checked before the grant (review F7, WA-7).** Every `Hello` reports the Router build's `schema_support`, and the registry keeps the last value per installation. A transfer to an installation whose support is unknown, or older than the copy's schema version, is refused `schemaTooNew`, naming both. The copy makes that comparison inside the fence transaction, against its own verified schema identity. The new holder's local `SchemaReady` check after seeding (PD2) is an additional admission check, not a substitute.
- **Fence before grant.** A crash after the fence and before the grant leaves the copy fenced with no lease for the new epoch; the registry still records the old lease. The copy already records the target as the new epoch's holder, because the fence sets both. Without a lease the target's Router admits nothing at that epoch, so no piece arrives at that epoch before the grant, and only that target can ever be granted it. The old holder may still admit writes locally until it learns it was superseded or its lease expires; its pieces are refused `staleEpoch` and the suffix becomes its rejected tail (spec 2 WA-5). The transfer stays `Fencing`. Repeating the same request carries the same `transfer_id`, gets the stored fence receipt, and completes. It never issues a second epoch (FL-10). The fence receipt also records the old epoch's cutoff (`start`), which the copy returns as `epoch_ended_at` in every later `staleEpoch` to that epoch (review F3).
- **Epoch floor after a restore.** If the registry is restored to an earlier state, the copy's fence is the second record of the highest epoch. A fence with `new_epoch ≤ fence` is refused with the current fence, and the registry retries once at `fence + 1`. If neither record can be read, the takeover is refused.
- **Move.** The registry records a `Draining` transfer and sends `ReleaseRequested` to the holder; the holder also sees it in every renewal answer (`transfer_in_progress`). The holder stops admitting, replicates everything, and answers with `Release{epoch, transfer, outcome}`. If a migration is pending, the outcome is `Refused(MigrationPending{migration, state})`: the registry records the transfer as refused `migrationPending`, leaves the lease unchanged, and the holder returns to `Current`. Otherwise the outcome is `Final(final)`, and the registry fences with `expect{epoch, acked: final}` and the target's schema limit, then grants with `start = final` (M3.3 finding: only the holder knows its migration state). If the holder hasn't renewed in the last 30 minutes (U45) or has no connected session, the move is refused as `holderUnreachable`; the admin can then choose a takeover.
- **Renewal.** Each attempt is recorded by its attempt ID in `renewal_attempts(attempt, project, installation, epoch, answer, decided_at)`, kept for the project's life, because FL-12 sets no replay window (review F16). At one attempt per project every 15 minutes, that's about 3.5 MB a year per project in the registry. Compacting old attempts would need a spec change that bounds replay, so it isn't done. A re-sent attempt gets its recorded answer with `replayed: true` and the lease's current state (F27). A fresh attempt is checked against the current lease: same holder and epoch renews (an expired lease with no transfer renews on the same epoch); a superseded epoch gets `staleEpoch` with its cutoff. Expiry is computed when needed from `expires_at` against the registry's clock, with no timers.
- **Import (FL-17, FL-15).** `admin_operations` takes `/admin/v1/import/projects/{id}` only on the owner route (FL-13). It calls the copy first, then the registry:
  1. `Copy: ImportSeed{project, digest, index, page, final_summary}`, one call and one transaction per page. Pages arrive in index order and are staged under `(digest, index)`. A non-final page answers `Staged{digest, index}`, a re-sent page with the same content is a no-op with the same answer, and a page with any other index is refused, naming the expected one. Staged pages are invisible: the copy stays not initialized until the last page arrives. A different digest while an earlier import is still staged discards that staging, since nothing was initialized. The last page recomputes the `ImportDigest` over the staged rows with the shared helper (PD2 step 4), checks it against `digest` and checks the schema identity. It then answers `Imported{head}` and sets fence = 1 (the import epoch), holder = none, start = `(1, 0)`, acked = the imported head, and `imported{digest, import_epoch: 1}`, and stores the page's `final_summary` in `_copy_summary` in the same transaction. After commit it dispatches `DirectoryUpdate{project, summary, position: imported head, acked_at}` to `FleetObject`, on the same alarm-repaired path as a piece's summary (§6), so an unheld project's status and latest-record time are listed before any claim (review F25). A copy marked retired answers `Retired` before any digest comparison (FL-18). A copy that's already imported with the same digest answers `AlreadyImported`. Any other existing copy answers `ImportConflict{existing: ExistingImport::{Imported(ImportDigest), NotImported}, offered}` and is untouched. `NotImported` covers a copy a holder created, which has no import digest to compare. Because the copy is one object, an import and a claim's fence never interleave.
  2. `Registry: RecordUnheld{project, import_epoch: 1, imported_head, schema_version}`. It writes the `Unheld` row, raises the project's `epoch_floor` to 1, and dispatches `DirectoryUpdate{project, lease: Unheld}`. It's idempotent, and a project the registry already has a lease or transfer for keeps it unchanged.

  A crash between the two leaves an imported copy the registry doesn't know: a claim answers `NotFound`, and re-running the import (the script's normal re-run) gets `AlreadyImported` from the copy and then completes step 2.
- **Fleet import (FL-17; M3.5 finding).** `/admin/v1/import/fleet` runs **before** any project import, on the owner route only. Its body is one bundle for the whole run:
  - **The bundle** is `FleetImportBundle { provenance: ImportProvenance, records: Vec<FleetImportRecord> }`, with `FleetImportRecord::{Workspace{id: WorkspaceId, name, description, repositories, time: RecordTime}, Board{id: BoardId, workspace, name, description, state: ImportedBoardState, time: RecordTime}, Directory{project: ProjectId, board, title, time: RecordTime}}`.
    - Original IDs, descriptions and the legacy board state are kept (review R6-1: "nothing invented" also means nothing dropped).
    - **Times (spec 1 RC-9; 2026-10-17 review F4).** The shipped store records no creation time for workspaces, boards or topics (`202609120001_project_board.sql`), so each record's `time` is `RecordTime::LegacyUnknown` (PD1 §3b). A known time is carried only where the source actually recorded one. The import never fills a time from an ID, the run's clock or a file time. The bundle's canonical digest covers the `time` variant, so a rerun at another wall-clock time gives the same digest.
    - `ImportedBoardState` maps the legacy state deterministically: `Active` becomes `Open`, and `Archived` becomes `Archived`. An imported board is always `kind: Area` (spec 1 §5), and the archived ones stay read-only for work (E2).
    - The records become the existing typed `Workspace`, `Board` and `ProjectDirectoryEntry` (PD1 E1–E3), so storage, the feed, snapshots and caches carry every field.
    - Nothing else is imported: the old store has no ADRs or process definitions (spec 1 §5).
    - **Provenance (M3.5 finding).** The legacy store records no author for workspaces or boards, so every imported fleet record is attributed to the person running the import. That's `ImportProvenance { installation: ServiceId, machine: MachineLabel }`, taken from the importing computer's Router identity, and each record's envelope gets:
      - `actor = RecordedActor::Human{ human: owner }`, the same declared owner identity the board uses elsewhere (spec 1 §6), never the owner's sign-in address;
      - `origin` = the `ImportProvenance` (PD1 §3), the importing computer's installation and machine;
      - `request: None`;
      - `time` = the record's `RecordTime`, stored as `router_time` (NULL for `LegacyUnknown`) and `time_basis`, and returned unchanged by the fleet feed, snapshots and Routers' caches;
      - the fleet position central assigns at import.

      This mirrors PD2's import mode, where project records keep their old author as `actor` and take the importing installation as `origin`; fleet records have no old author to keep. Provenance is recorded on the first import and **isn't part of a record's digest**, so an identical record re-imported from another computer is `AlreadyImported`, not a conflict.
  - **Digests.** Each record's digest is SHA-256 over its canonical JSON (every field above, so a changed description or state is a different digest), the same canonicalization as `ImportDigest`. The bundle's digest, over the sorted record digests, is logged only.
  - **Admission is one pure rule**, `fleet_admission::decide_import(&FleetImportBundle, &FleetSnapshot) -> FleetImportDecision`. It lives in the shared `fleet-admission` crate beside `decide`, reads only the keys `reads_for_import` names, and does no IO. For each record, parents first:
    - **absent:** insert it, after the bundle's parents and the existing records check out;
    - **present and imported with the same digest:** `AlreadyImported`;
    - **present with a different digest:** `ImportConflict{existing: ExistingImport::Imported(digest)}`;
    - **present and created by ordinary admission:** `ImportConflict{existing: NotImported}`;
    - **a missing parent:** `ParentMissing{parent}`;
    - **a name another record already uses in its scope:** `NameConflict{existing}` (the script refuses collision-bearing runs first; F19).
  - **All or nothing.** `FleetObject` runs the decision and its writes in **one** transaction. If any record is refused, nothing is written, and the answer `FleetImportResult::Refused{conflicts: Vec<FleetImportConflict>}` lists every conflict. Otherwise the answer is `Imported{counts}`, or `AlreadyImported` when every record replayed.
  - **Recording.** Imported records store their `import_digest` (an `import_digest` column on the FleetObject's own fleet tables; the M2 local stand-in never imports), and the transaction emits their fleet-feed entries (FL-6). So Routers' fleet caches see them.
  - **Unheld state.** The directory entries the bundle creates are listed `Unheld` once each project's import has run `RecordUnheld` (above).
  - **Re-running** is idempotent: an identical bundle answers `AlreadyImported` and writes nothing.
- **Claim (FL-16; review F20).** A claim is a recorded transfer of kind `Claim`, keyed by its attempt ID, on the same fence-then-grant path as a takeover:
  1. The claimant's `ConnectionObject` dispatches `Registry: Claim{attempt, project, claimant}`, where `claimant` comes from the authenticated session.
  2. The registry checks, in order:
     - a `Retired` tombstone answers `Retired` (FL-18). This precedes even the replay of an earlier attempt, as FL-18 states;
     - a transfer recorded for this attempt resumes or replays it, so a re-sent attempt never issues a second epoch;
     - an active `Retire` transfer answers `TransferInProgress{transfer}` to a **new** attempt. A repeated earlier attempt has already replayed its recorded answer in the step above. During a retirement, that answer can only be a refusal: retirement requires no lease and no transfer, so no granted or in-progress claim can exist then;
     - an active holder change or claim answers `AlreadyHeld{holder: transfer.to}`, so of two racing claims the later gets the winner;
     - a lease row answers `AlreadyHeld{holder}`, including just after a claim's grant removed the `Unheld` row;
     - only then does a missing `Unheld` row answer `NotFound`: the registry knows no such project (review F24).

     Only the import creates `Unheld`, so a project whose takeover is still fencing (its old lease still recorded, no lease for the new epoch yet) is never claimable. Next, the claimant's last reported `schema_support` must be known and cover `schema_version`; otherwise `SchemaTooNew{store, supported}`. Then `new_epoch = max(epoch_floor, import_epoch) + 1`, and the transfer is recorded as `Reserving`.
  2a. **Fleet reservation** (M3.5 finding: archive finalization must see claims). `Fleet: ReserveClaim{project, attempt}`, registry → fleet.
     - **Archiving or archived board:** Fleet refuses with `Refused(BoardArchiving | BoardArchived)`. A claim exists to write, and an archiving board is read-only for work. The registry records the transfer as refused, answers `Archived`, and fences nothing.
     - **Otherwise:** Fleet records `claim_reservations(project, attempt, state: ReservationState::{Active, Settled{outcome: Granted | Refused}})` in its own bookkeeping, with state `Active`, and answers `Reserved`; the transfer moves to `Fencing`.
     - **The lifecycle is monotonic per `(project, attempt)`** (review R13-1):
       - a re-sent `ReserveClaim` for an `Active` attempt answers `Reserved`;
       - for a `Settled` attempt it answers `AlreadySettled{outcome}` and never makes it active again;
       - a `ReleaseClaim` that arrives before its `ReserveClaim` records `Settled{Refused}` straight away, so the late reserve answers `AlreadySettled`;
       - releasing twice is a no-op;
       - cleanup is keyed to the exact attempt, so a stale release or update never touches another attempt's row.

       Settled rows are kept for the project's life; they're tiny.
     - **Settlement:**
       - **Grant.** The grant's lease `DirectoryUpdate{lease: Held}` settles the attempt `Granted` in the **same** Fleet transaction that applies the update. So finalization never sees an in-between state of `Known(Unheld)` with no reservation.
       - **Refusal.** A terminally refused claim (`PreviewStale`, `SchemaTooNew` or `Archived`) sends `Fleet: ReleaseClaim{project, attempt}`, which settles the attempt `Refused`. A claim isn't abandoned on a timeout: a transfer still in `Fencing` can always be resumed by its re-sent attempt, so it keeps its reservation.
       - **Cleanup** is durably owed. The transfer record carries `fleet_cleanup: Owed | Acknowledged`, and the registry's alarm resends it until it's acknowledged.
  3. `Copy: Fence{transfer_id: attempt, new_epoch, holder: claimant, expect{epoch: import_epoch, acked: imported_head}, target_schema_max}`, the same fence as a takeover. Its transaction sets fence = `new_epoch`, holder = claimant and start = the imported head, and records the receipt. If the copy's fence is no longer the import epoch, because the project was claimed before a registry restore, it answers `PreviewStale` with the current fence. The registry reads that epoch's holder with `CopyStatus`, answers `AlreadyHeld{holder}`, records the transfer as refused and grants nothing.
  4. On `Fenced{start}`: lease = `{claimant, new_epoch, now + 4 h}`, `epoch_floor = new_epoch`, transfer = `Granted`, and the `Unheld` row is removed. The answer is `Claimed{epoch, expires_at, start}`, and `LeaseGranted` goes to the claimant. A crash after the fence and before the grant leaves the transfer `Fencing`; the re-sent attempt gets the stored fence receipt and completes the same grant.

  After the grant, the project is held like any other: renewals, `Welcome`, moves and takeovers follow the normal path. The claimant seeds from the copy at `start` before it admits anything (PD2).
- **Creation.** The creating machine's **session** sends the directory entry to `FleetObject` (session → fleet). When Fleet answers that it accepted it, the **session** then asks the registry to reserve epoch 1 for its installation (session → registry). Fleet never calls the registry (the typed DAG). If the session's answer is lost, the Router re-sends the creating request: Fleet's acceptance replays, and the reservation is idempotent per project. The registry initializes the copy with fence 1 and start `(1, 0)`. Until then the copy answers `notInitialized`, so central never holds data for a project it didn't accept (WA-10).

## 6. Other paths

- **Fleet requests (WA-1).** A Router sends a `FleetRequest` with its request ID over the session. `FleetObject` runs `fleet-admission`: ledger, revision, rules, then the write with its feed entry, in one transaction. Routers see the result in their fleet cache from the feed. While central is unreachable, the Router keeps the request `Pending` and answers `pendingUnreachable`.
- **The fleet feed and notices (FL-6, C10, C11).** There are two durable streams, each with one owner. A push is only a hint, and correctness never depends on it (M3.5 finding).
  - **The fleet feed** belongs to `FleetObject`: `fleet_feed(position: FleetPosition, record, as_of)`. Every fleet write appends to it in its own transaction.
    - **An entry's identity is its `position`**, allocated in that transaction as the next integer key (M3 finding: the earlier `event_id` had no consumer). So any number of entries in one step needs no ID supply from the shell.
    - **`as_of`** is central's time when the entry was appended, used only for retention. A record's own envelope time, an imported record's original time included, stays in `record`.
    - **Reading.** A Router reads it with `Read{scope: FleetRecords, after: FleetPosition}`: after `Welcome`, whenever it gets a hint, **and every 60 seconds while connected, whatever the hints do** (review R15-1). So a lost hint delays the cache by at most that interval, and never strands it. The cursor it carries is the last fully applied position.
    - **Hints.** Fleet keeps `feed_readers(installation, last_read_position)`, updated by each read. **Every Fleet → delivery dispatch runs from Fleet's own alarm, never inside an inbound call** (review R15-4).
      - A mutation commits its owed hints and notices to Fleet's `notice_outbox(installation, kind, owed_at)` in its own transaction, and sets the alarm.
      - The alarm, a later separate event, sends each one as `Delivery: Notice{kind}`: one hop, retried until the delivery face acknowledges.

      So no inbound chain goes past Fleet, and session → registry → Fleet stays two hops even when a claim's cleanup finalizes an archive.
    - **Hints are numbered notices** (review R15-2). A `FleetFeedAdvanced{through}` hint takes the same per-installation `NoticePosition` as any other notice: the delivery face appends it to `_session_notices` and writes `CentralFrame::Notice{position, kind}` if connected. `through` stays the separate fleet cursor.
    - **Retention.** Feed entries are kept for 30 days. A cursor older than that gets `cursorExpired`, and the Router re-reads the fleet records in full (FL-6).
  - **Machine notices** belong to each installation's `ConnectionObject`: `_session_notices(position: NoticePosition, kind: NoticeKind, created_at, expires_at)`.
    - **Appending.** Registry and fleet send notices to the delivery face (`Delivery: Notice{kind}`, edges registry → delivery and fleet → delivery). It appends them in its own storage, assigns the next per-installation `NoticePosition` (never 0), and writes `CentralFrame::Notice{position, kind}` if the installation is connected.
    - **Reading.** A Router reads `Read{scope: Notices, after: NoticePosition}` from its own session face, which reads the same object's storage with no self-call.
    - **Retention.** Notices are kept for 7 days. A notice is only a hint (FL-6: the machine acts on request answers), and a cursor older than retention gets `cursorExpired`, after which the Router renews its leases and re-reads fleet records.
    - **Who sends what.** The registry sends `LeaseSuperseded`, `LeaseGranted` and `ReleaseRequested`; Fleet sends `BoardArchiving` to each holder installation it knows for the board's projects (a holder that's missed is still covered by the feed's `Archiving` record) and `RejectedTailReported` to the current holder.
- **Rejected-tail reports (FL-8).** **The report is retrievable, not just announced** (review R15-3).
  - `Read{scope: TailReports{project}}` returns every stored `TailReportRow{project, epoch, request_ids, digest}`.
  - The project's current holder reads it on acquiring the project (after `Welcome`, a claim, a move or a takeover grant), and again every 60 seconds while it holds the project, the same as the fleet feed.
  - It records each unrecorded report as its `RejectedTail` fact, idempotently by `(project, epoch)`.
  - The `RejectedTailReported` notice only speeds this up. The holder captured at `VerifyTail` is never the only way the report reaches the current holder.
  - A conflicting digest for a stored `(project, epoch)` never overwrites the first report.

  A superseded holder sends `TailReport{project, epoch, request_ids, digest}`. Under the DAG this is **two sequential session calls**, like creation, so every chain stays 2 hops (M3.5 finding):
  1. session → registry `VerifyTail{project, epoch, installation}` answers `Verified{current_holder}`, or `Refused(NotWriter)` when that epoch was never this installation's;
  2. only on `Verified`, session → fleet `StoreTail{project, epoch, request_ids, digest, current_holder}`.

  `FleetObject` stores the report idempotently, keyed by `(project, epoch)` with the digest checked, and sends `Notice{RejectedTailReported{project, epoch}}` to the current holder's delivery face. That holder's `project_admission` records the `RejectedTail` fact (PD2 §6).
- **Archive (FL-5).** **Finalization's "no holder" test (M3.5 finding):** a project counts as having no holder only if its lease side is `Known(Unheld)` **and** it has no claim reservation. A reserved project counts as held, so the board waits for its report. **Every Fleet mutation that can complete the predicate runs it in the same transaction** (review R13-2): an `ArchiveSeen` report, a reservation settled `Refused`, and a lease update that leaves a project `Known(Unheld)`. If that mutation removes the board's last blocker, the board, and its workspace when that was the workspace's last board, become `Archived` exactly once, with the existing attribution (each `Archiving` record's actor, `request: None`, central time, feed entries). Replays find the board already archived and write nothing. Fleet owns both facts, and the registry reaches Fleet before any claim can fence (§5 Claim step 2a), so a claim can't slip past finalization through a lagging directory update. `FleetObject` marks the board `archiving` and emits a notice. Each holder refuses work writes to the board's projects (the `Archived` gate, with spec 1 E2's reader exceptions still admitted), records `ArchiveSeen` in the project, and, once that fact is acknowledged, sends `ArchiveSeen{board, project, position}`. The board reads `archived` once every project has reported or has no holder. Fleet admission's "parent must be active" gate exempts exactly this report: `ArchiveSeen` is accepted while its board, or its board's workspace, is `archiving`, with all its board, project and report checks kept. When the last board of an archiving workspace becomes `archived`, the same fleet transaction marks the workspace `archived` (FL-5; M2 corrections I). No other operation gets the exemption. A restart resumes from the stored reports. `ProjectCopyObject` keeps applying an archived project's pieces under the normal fence, so allowed reader-state changes replicate; its `archived` flag is for reads and views, never a piece refusal.
  - **The `ArchiveSeen` report is an automated report, not a fleet request** (M3.5 finding), the same as `DirectoryUpdate`.
    - **No ledger entry.** It carries no request ID and no `RecordedActor`, and it writes no request-ledger entry.
    - **Storage.** `FleetObject` stores it in its own bookkeeping, `archive_reports(board, project, position, installation, reported_at)`, keyed `(board, project)`. The installation comes from the authenticated session. The first report wins, and a repeat, from the same holder or a successor, is a no-op that answers the same acknowledgement.
    - **Decision.** A pure rule, `fleet_admission::decide_archive_report(&ArchiveReport, &FleetSnapshot) -> ArchiveReportDecision{record: bool, finalize: Option<ArchiveFinalization{board, workspace: bool}>}`, sits beside `decide`. It applies the existing parent-gate exemption and decides whether this report completes the board, and with it the workspace.
    - **Attribution.** The finalizing writes are the board's state `Archived`, and the workspace's when its last board finishes. Each is an updated typed record whose envelope actor is the actor of the `Archiving` record it completes, so the archive's requester is credited with its completion. Its `origin` (PD1 §3) is that same `Archiving` record's stored `origin`, the requester's verified installation and machine. It isn't re-derived from whichever report or completion path triggered it; only its position and time are new. It has `request: None` (no new decision), central's time, and its own feed entry. No identity is invented, and the original archive request's ledger decision is unchanged.
- **Metadata (FL-1, FL-2, U46; review F10).** Each part has one source and its own freshness:
  - **Name, workspace and board:** fleet admission writes them with the directory entry, and renames are fleet requests. They're current as of the fleet feed.
  - **Status and latest record time:** central doesn't parse board rows; it stays schema-agnostic. Instead every complete piece carries `summary: ProjectSummary{status, latest_record_at}`, derived only from the piece's own captured prefix: status as of its last transaction, and the Router time of its last record. A part of a larger transaction carries none, and its final part carries it, so a rebuilt piece always has the same summary and digest (PD2 §3; review F10). The copy stores it in `_copy_summary` in the same transaction that applies the piece. An imported copy gets its first summary from the import's last page instead (§5 Import); an `AlreadyImported` answer touches neither the copy nor its summary. After commit, the copy dispatches `DirectoryUpdate{project, summary, position, acked_at}` to `FleetObject`.
  - **Holder and lease state:** the registry dispatches `DirectoryUpdate{project, lease, lease_version, as_of}` after every lease change and renewal, with its own `as_of`.
    - **Ordering uses `lease_version`, not time** (M3.5 finding: two updates can share a registry timestamp).
    - `lease_version` is `LeaseVersion{epoch: EpochNumber, incarnation: RegistryIncarnation, change: u64}`, ordered epoch, then incarnation, then change.
      - `change` is a per-project counter the registry persists with the lease or `Unheld` row. It's incremented in the same transaction as every lease change (`RecordUnheld`, a grant, a renewal, a release, a supersession and a retirement), and a new epoch restarts it at 1.
      - `incarnation` belongs to the registry's current run, not to a lease change, and is stamped on each update when it's dispatched.
      - Each update also carries a digest of its lease content.
    - **Fleet issues the incarnations** (reviews R19-1 and R20-1).
      - A registry run that has just started has no incarnation and publishes nothing until it gets one.
      - It sends `AdvanceLeaseIncarnation{at_least}` to `FleetObject` (the existing registry → fleet edge), where `at_least` is the last incarnation the registry persisted.
      - In one transaction Fleet sets `lease_incarnation_fence = max(fence, at_least) + 1` and returns it. The registry persists it as its last incarnation and uses it for the rest of the run.
      - A lost answer is asked again. The number that went unused is skipped, never reused.

      This gives three guarantees:
      - **Every run outranks every earlier run.** Each incarnation is handed out by the same fence, so a run's incarnation is above every incarnation an earlier run used. A registry restore can't break this: it takes effect only when the object restarts, which starts a new run, and the fence lives in Fleet.
      - **Late updates from earlier runs can't land.** Fleet refuses any lease update below the fence as `Fenced{fence}` and applies nothing. So an update dispatched by an earlier run, including one from before a restore, can't land once the current run has its incarnation.
      - **Nothing has to detect a restore.** A restored registry can reuse `change` values, but under a new incarnation, so at the same epoch its updates always outrank the old ones.

      If Fleet can't be reached, the run publishes nothing. The directory's lease side ages, with its `as_of` showing how old it is, and lease decisions and renewal answers carry on unchanged (FL-12).
    - **Fleet answers** each lease update with `LeaseUpdateResult::{Applied, Duplicate, Older{stored: LeaseVersion}, Fenced{fence: RegistryIncarnation}, Conflict{stored: LeaseVersion}}`. The registry handles every answer against its own current state, never by reasoning from the old offer (review R20-2):
      - **`Applied`, or `Duplicate`** (the same version and digest): recorded as acknowledged.
      - **`Older`:** the offer was out of date, so it's dropped and nothing starts. It may simply have arrived late. Or Fleet may hold a higher epoch, which is the epoch floor's business (FL-9) until the floor issues the project's next epoch.
      - **`Fenced` or `Conflict`:** `Fenced` means this run's incarnation is no longer current. `Conflict` means one version carries two contents, which can only happen if Fleet itself was restored behind its fence. Either way, the registry takes a new incarnation as above, with `at_least` the larger of its own and the stored one, and republishes its current state.
    - **Replays publish nothing.** A renewal replay changes no lease state, so it allocates no version and sends no update of its own (FL-12). The repair loop below only ever sends a project's current version.
    - **After a restore, Fleet can be ahead for a project.** Its lease side can keep the state the registry last published before the restore, newer than the restored registry's. It stays until that project's next lease change, which for a held project is at most one renewal interval. Holders never act on it (FL-12).
    - **Not covered:** the registry and Fleet both restored behind the same fence. This is the metadata form of FL-9's case where neither record can be read.
    - `as_of` stays display freshness only.
    - A claim's grant update is `DirectoryUpdate::ClaimGranted{project, attempt, lease, lease_version, as_of}`. In one Fleet transaction it applies the lease side, if its version is higher, and settles that attempt `Granted`.
  - `FleetObject` keeps the newest update per side, by position for the copy side and by `lease_version` for the lease side, so a late or repeated update never moves a side backwards.
  - An interrupted update is repaired by the copy's alarm: if `_copy_summary` is newer than the last update `FleetObject` acknowledged, it sends it again. The registry does the same for the lease side.
  - Search filters on these columns, and each result carries both sides' freshness. Anything more is read from the copy (C19), as a read-only snapshot labelled with its position.

## 7. Failure, recovery and concurrency

| Failure | Detected by | Contained to | Recovery and owner |
|---|---|---|---|
| A Router can't reach central | Router session | that machine | pieces and fleet requests wait; writes continue until the local lease deadline, then pause (PD2 §5). Owner: Router |
| A deploy closes every WebSocket | Router session | in-flight frames become unknown outcomes | Routers reconnect and `Hello`; `Welcome` returns each project's acknowledged position; nothing in memory matters (FL-11). Owner: `machine_session` |
| An object loses its memory (hibernation, eviction, restart) | — | — | every object rebuilds from its storage on the next event; socket mapping comes from the attachment; lease expiry is computed from storage |
| A takeover interrupted between fence and grant | transfer state `Fencing` | that project: copy fenced to the new epoch, no lease for it yet; the old holder's pieces refused | the admin repeats the request; the same `transfer_id` completes it. Owner: `LeaseRegistryObject` |
| Two transfers for one project at once | `transfers` (one active per project) | the second request | refused as `transferInProgress` |
| The registry restored to an earlier state | the copy refuses a fence at or below its own | that transfer | retry at `fence + 1`; refuse if neither record can be read |
| A lease update from an earlier registry run, including one from before a restore, arrives late | Fleet's `lease_incarnation_fence` | that update | refused `Fenced` and not applied; the current run's repair loop publishes the current version (§6 Metadata). Owner: `FleetObject` |
| A central copy is full | storage-limit error inside the transaction | that project | `centralCopyFull`; the owner raises the plan limit or archives (G21) |
| A large transaction exceeds the CPU limit | the event fails; the Router sees an unknown outcome or error repeat | that project | the Router stops and reports `pieceApplyFailed` (G17) |
| A machine credential leaks | owner | that machine | the admin revokes the machine; its sessions are refused; its leases expire and can be taken over |
| An Access service token leaks | owner | that machine's network gate | revoke or rotate the token in Access. The machine credential is still required, so a leaked token alone opens nothing |
| The owner's sign-in is compromised | owner | moves and takeovers | revoke Access sessions for the owner and secure the Google account; there's no stored admin secret to rotate |
| A panic in an object | the request hangs; the instance restarts | that event | writes are only ever made inside a transaction; panics are denied by lint. A panic *inside* the callback is untested (G18) |

**Concurrency.**
- Each object runs one event at a time, so different projects proceed in parallel in different copy objects.
- While an object awaits a call to another object, other events may run. The Router sends one piece per project at a time and waits for its ack, so pieces don't overtake each other. The `gap` check catches it if they ever do.
- A renewal that arrives while a takeover awaits the fence can succeed for the old epoch. That does no harm: the copy is already fenced, so the old holder's next piece is refused and it learns it was superseded.

## 8. Proof seams

| Seam | Real | Stand-in | Observes |
|---|---|---|---|
| `collaboration-central-local` natively, with two Router stores (V3, V9) | the core and the local adapter: real Turso stores, real WebSocket sessions | lease length and staged-part age shortened by configuration; the same test-only fault hook | every FL check, fast and without Cloudflare |
| `wrangler dev` (workerd) with the real `collaboration-central-cloudflare` and two Router stores | all four object kinds, the edge, the wrapper, Router capture and session | lease length and staged-part age shortened by configuration; a test-only fault hook (compiled only in a test build) that stops the registry after the fence | V9: every FL check in spec 3, including a fence that survives a workerd restart, the interrupted takeover finished by repeating it, `adminRequired`, a refused fence at or below the copy's, and session resume |
| `collaboration-central-core` on its own, and its conformance suite on both platforms | the core's decide functions; then each platform's shell and `SqlTx` | fixed times and IDs passed as arguments | fast native tests of every apply decision, the fence, lease expiry and renewal, the epoch floor, and the transfer steps, including an interrupted takeover and its repeat |
| `storage_transaction` on the official `Storage::transaction()` (a real integration test, with no mocks; owner, 2026-10-11) | the compiled Worker (`worker-build`, wasm32) under real workerd (`wrangler dev`, local mode), with real Durable Object SQLite storage. Then the same test against a deployed staging account (V9d) | none: no mocked storage and no fake JavaScript API | each case runs through a real request, and the persisted state is read back through a **separate** request and again after **restarting workerd**: <br>• a SQL constraint error inside rolls back every write; <br>• a Rust `Err` inside rolls back every write; <br>• a fence mismatch rolls back the row and leaves the fence alone, while the matching epoch commits rows and position together; <br>• a panic inside the callback (records what happens; G18); <br>• no other request to the object can observe a half-applied transaction. <br>It's a permanent test in the repo with its own CI job, not a one-off probe |
| Deployed staging | a Cloudflare staging account | — | V9d: fence, takeover crash cases, and resume across a real deploy |
| Cross-machine | — | — | V7, unverified until P1 and P5 exist |

## 9. Trace

| U | R | E | Owner | Interface | Shape and home | State | Failure | Proof |
|---|---|---|---|---|---|---|---|---|
| U20 | WA-1 | E1, E2, E7, E16, E23 | `FleetObject` + `LeaseRegistryObject` | `FleetRequest` frame | `fleet-admission` port; fleet ledger | Pending on a Router while unreachable | `pendingUnreachable` | V7, V9 |
| U24 | FL-1 | E3 | `FleetObject` | directory and search frames | directory row with freshness | — | — | V9 |
| U21, U24 | FL-2 | E3, E19, E24 | `ProjectCopyObject` | `SnapshotPage` | read-only, with its position | — | — | V9 |
| U21, U25, U37 | FL-3 | E20, E24, E25 | `ProjectCopyObject` | `apply`, `fence` | `_copy_state` | fence only increases | `staleEpoch`, `gap` | V9, V9d |
| U32 | FL-4 | E24 | `ProjectCopyObject` | `apply` | — | — | `centralCopyFull` | V9 |
| U3, U20 | FL-5 | E2, E13 | `FleetObject` + holders | `ArchiveSeen` frame | archive reports | active → archiving → archived | — | V9 |
| U55, U57, U58 | FL-13 | E26 | `edge` + `LeaseRegistryObject` (enrollment client ID) | every request; `/v1/session`; `/admin/v1/*` | `AccessCaller::{Machine{client_id}, Owner{email}}` | — | refused at the edge; `Closed{CredentialRejected}` | V9, V9d |
| U56 | FL-14 | E26, E10 | sender and holder `ConnectionObject` + `LeaseRegistryObject` (`ForwardRoute`) | `Forward` → `ForwardRoute` → `DeliverForward` → `ForwardAnswered` | `pending_forwards`, `delivered_forwards` (transient) | pending → answered · unanswered · unreachable | `senderHolds`, `notHeld`, `holderUnreachable`, `unanswered`, `overloaded` | V7, V9 |
| U54 | FL-15 | E3, E23, E24 | `LeaseRegistryObject` (`Unheld`) + `ProjectCopyObject` | `Read{scope: ProjectCopy}`; writes and `Forward` | the registry's `Unheld` lease row; the copy at the imported head | unheld → held, by claim only | `notHeld` | V9 |
| U54 | FL-16 | E20, E23, E24 | `LeaseRegistryObject` (`Claim`) through `ConnectionObject`; the Router's `project_seeding` (PD2) | `SessionRequest::Claim{attempt, project}` → `Registry: Claim` → `Copy: Fence` → `Claimed{epoch, expires_at, start}` | `transfers` (kind `Claim`, keyed by attempt ID), the copy's fence receipt, then `leases` | unheld → fencing → held at `max(epoch_floor, import_epoch) + 1`; a crash while fencing resumes on the re-sent attempt | `AlreadyHeld{holder}`, `SchemaTooNew{store, supported}`, `NotFound` | V7, V9 |
| U54, U57 | FL-17 | E1, E2, E3, E24 | `admin_operations` + `FleetObject` (import path) + `ProjectCopyObject::ImportSeed` | `/admin/v1/import/fleet`, `/admin/v1/import/projects/{id}` | fleet records with their original IDs; an `Unheld` copy at the imported head | absent → unheld | `AlreadyImported`, `ImportConflict{existing, offered}`; owner-only at the edge (FL-13) | V4, V9 |
| U9 | FL-6 | — | `FleetObject` + `ConnectionObject` | notices, feed by cursor | outbox row, cursor | — | `cursorExpired` | V9 |
| U9 | FL-7 | E21, E25 | `ProjectCopyObject` + Router `machine_session` | `apply`, `Hello` → `Welcome` | acked position | — | unknown outcome → resend (no-op if applied) | V9 |
| U11, U21 | FL-8 | E22 | `FleetObject` + `LeaseRegistryObject` + holder `project_admission` | `TailReport` frame | `tail_reports`; `RejectedTail` fact | — | — | V9 |
| U37, U38 | FL-9 | E23 | `LeaseRegistryObject` | renew, reserve first epoch | `leases`, `epoch_floor` | held → expired → held \| superseded \| released | `NotHolder`, `Superseded` | V9 |
| U39 | FL-10 | E23 | `admin_operations` + `LeaseRegistryObject` + `ProjectCopyObject` | `/admin/v1/projects/{id}/move \| takeover` | `transfers`; `TakeoverPreview`; fence receipt | Requested → Draining/Fencing → Granted \| Refused | `adminRequired`, `holderUnreachable`, `migrationPending`, `transferInProgress` | V7, V9, V9d |
| U34, U35, U36 | FL-11 | E26 | `ConnectionObject` | `/v1/session` WebSocket | `MachineFrame`, `CentralFrame`; attachment | connected · resuming · closed | resume after a drop or deploy | V9, V9d |
| U34, U36–U39 | FL-12 | E26, E23, E27 | `edge` + `ConnectionObject` | `Hello`/`Welcome`, `Request`/`Answer`, `Notice`, `Closed` | PD2 §3 frame enums | connected · closed (`ConnectionReplaced`, `CredentialRejected`) | `credentialRejected`, `connectionReplaced`; lost answer → re-send | V9 |
| U67 | — (structure) | E24 | `admin_operations` + `FleetObject` + `ProjectCopyObject` + `LeaseRegistryObject` (retire) | `/admin/v1/copies…`, `/admin/v1/projects/{id}/retire` (§11) | `Health`, `CopyOpsCall`, `_copy_migrations`, `_central_migrations`, transfer kind `Retire` | bookkeeping current \| migrationPending; Checking → Wiping → Directory → Retired | `migrationPending`, `previewStale`, `retired` | V9, V9d |
| U68 | — (structure) | — | `collaboration-central-core` call graph | — | `ObjectKind`, `MayCall<To>`, `VerifiedCaller`, edge effects | — | `CallEdgeRefused` | compile-fail tests *(replaced 2026-10-16: the compiler gate checked in review, plus V-OC3's wire refusal of a forged caller, per U74; see the central one-core spec, OC-21)*, V9d |

## 10. Open

- **G4, settled (U53):** this lane builds the registry and admin endpoint, verified on real paths by a separate verification Sidekick (V9, V9d). P1 keeps session placement and routing roles to sessions. Machine identity is the installation. The Host's existing singleton lock (`host_singleton_authority`) means one Router runs per router root, so a separate process "incarnation" isn't needed. P1 can add one if it places several processes per installation.
- **G11:** closed by U46; the metadata path is in §6.
- **G17, G18:** see spec 2.
- **G20, settled (U57):** admin access is the owner's Google sign-in through a separate Access app covering `/admin/v1/*`. The CLI gets a short-lived Access token through `cloudflared`. There's no stored admin secret.
- **G21 (owner):** the Cloudflare plan (Free: 1 GB per object; Paid: 10 GB) and the account to deploy into.
- **Assumption, checked in V9:** Durable Object SQLite runs every project migration as written for Turso.
- **Constraint, found on staging 2026-10-15 (M3.10): hosted call-depth budget.** On Cloudflare, chains of object-to-object calls, including callbacks into a caller's object during one request, count against a platform loop and depth budget. Local workerd enforces none of it. The documented Worker loop limit is 16 `CF-EW-Via` invocations; no separate Durable Object threshold is published. The fault build's per-call registry self-RPCs exceeded it, while production paths reach at most 2 levels (takeover, move, claim, floor repair) and forward reaches 1. **Rule:**
  - keep production chains shallow;
  - never call back into the calling object within the same request;
  - any future path deeper than 2 (the Fleet receiver, the message ledger) needs a hosted measurement before it's accepted.

  **Measured on staging (2026-10-15, M3 fault build, a temporary instrument since removed):** chains of 15 Durable Object entries from the authenticated edge completed, and the 16th was refused with `Subrequest depth limit exceeded`. This held both for fresh objects and for a deliberate two-object re-entry. That's consistent with a 16-invocation budget that counts the edge Worker. The DAG's longest chain (4) and every production path (2 or fewer) sit well inside it.

  Evidence: the M3.10 depth diagnosis in the implementation worktree.
- **Object-to-object calls form a typed, ranked DAG,** enforced at compile time (owner, 2026-10-15). Each object kind has a rank and its own call, result and effect types. A handler can only return effects addressed to the kinds it may call, and an object never calls back down the chain. The design defines one marker trait, `MayCall<To>`, implemented only for allowed edges, with a compile-time assertion that every edge goes up in rank. The receiver also checks the caller kind carried in the wire envelope, as defence in depth. Ranks and edges:

  | Rank | Kind | May call |
  |---:|---|---|
  | 0 | edge (Access-verified Worker fetch) | session, registry, copy, fleet |
  | 1 | session (`ConnectionObject` handling its machine's frames) | registry, copy, fleet, delivery |
  | 2 | registry (`LeaseRegistryObject`) | copy, fleet, delivery |
  | 3 | copy (`ProjectCopyObject`) | fleet |
  | 4 | fleet (`FleetObject`) | delivery |
  | 5 | delivery (`ConnectionObject` receiving notices or forwarded posts) | **nothing.** Only local effects: socket writes, its own storage, and **its own alarms** (for example the cleanup of `delivered_forwards`, PD3 §2a). An alarm is a later, separate platform event on the same object, not an object-to-object call, and its handler runs with the effect type of the face that set it |

  The `ConnectionObject` has two faces, session and delivery, with separate call types, so the delivery face can't start calls. The longest possible chain is 4 hops; today's production paths use at most 2. Test-only conformance controls obey the same types, so they can't call back up the chain either. New objects, such as the message ledger and the M3.5 Fleet receiver, get a rank and explicit edges before they're built.
- **The entrypoint (rank 0) is typed by the verified caller class.** Access verification yields exactly one `VerifiedCaller::{Machine(EnrolledInstallation), Owner(OwnerIdentity)}`, and each class has its own effect type:
  - `MachineEdgeEffect::{OpenSession(InstallationId)}`: `/v1/session` can only open that installation's session face;
  - `OwnerEdgeEffect::{Registry(RegistryAdminCall), CopyImport(ProjectId, CopyImportCall), FleetImport(FleetImportCall), CopyOps(ProjectId, CopyOpsCall), FleetDirectory(FleetDirectoryCall)}`: `/admin/v1/*` can only issue admin calls. `CopyOpsCall::{Health, Repair, Migrate}` and `FleetDirectoryCall::{Page{after, limit}}` serve §11's operations view. Retirement is `Registry(RegistryAdminCall::Retire{project, expect_fence, expect_acked})`, which goes on to call fleet `ArchiveStatus` and `MarkRetired` and copy `Retire`, all along existing upward edges.

  No admin call can be constructed from a machine caller, and no session from an owner caller. That makes FL-13's "a machine assertion never works on the admin route" a compile-time property. The route chooses the type once, at the boundary.

## 11. Operating the copies (U67)

There is one copy per project, so a fleet holds many objects. They're operated as one set, with no dormant object left on an old version.

**Each object kind's own bookkeeping is versioned.**
- **Copies** record it in `_copy_migrations(version, applied_at, from_version)`, plus a one-row `_copy_migration_failure(version, reason, at)`. Both are inside the reserved `_copy_*` namespace (PD2 §5's single classifier), so they never enter the project fingerprint, a snapshot or a schema step.
- **Other kinds**, which hold no replicated schema, use `_central_migrations` with the same columns.
- Each kind has an ordered migration list, which replaces today's `bootstrap_copy` (hard cutover).
- **One tolerant metadata read** (review R7) is used by the prelude, `Health` and `Migrate` alike. It checks `sqlite_schema` before it queries any table, and answers one of:
  - `Uninitialized`: no bookkeeping;
  - `Unrecorded{shape_v1: bool}`: bookkeeping tables but no `_copy_migrations`. This is adopted as version 1 only when the tables match the shape `bootstrap_copy` created; otherwise it's reported as damaged;
  - `Recorded{version}`.
- The migration prelude runs inside the first `storage_transaction` of an event. For a `Recorded` copy at the current version, the tolerant read costs two statements, a `sqlite_schema` probe and one version read (M3.12 finding: SQLite can't prepare a query against a missing table, so one guarded statement isn't possible), with no cache and no new port. Adoption (`Unrecorded`) adds the raw shape reads it needs. A failed migration rolls back that event, which answers `migrationPending{kind, version}`.
- **The copy's gate** (M3.12 finding; reviews R31-1 and R31-3). Every copy call runs these steps in order, and the first that applies answers:
  1. **Retired.** The tolerant read checks the retired marker in `_copy_state` first. A retired copy answers `Retired` to pieces, imports and reads, and replays its stored retire receipt to a repeated `Retire`, so an interrupted retirement can finish. It's never migrated or adopted.
     **Operations on a retired copy** (M3.12 finding). Each is a closed result:
     - **`Health`** stays readable, so an operator can see what happened. Its body is `CopyHealth::{Live(…), Retired{transfer, retired_at}}`, served as HTTP 200. Both fields are read from the retire receipt.
     - **The migration sweep** reports a closed `Retired` item, alongside `Migrated`, `Current` and `Failed`, and continues. The copy is never adopted or migrated.
     - **`Repair`** answers `Retired` and runs no alarm duties.
     - **Every other admin call**, including an import, answers HTTP 410 with the typed body `{kind: retired}`.
     - **A stale alarm** on a retired copy does nothing and doesn't re-arm.
  2. **Migration admission,** the prelude, for a live or uninitialized copy.
     - A storage-limit error anywhere in it answers `Unavailable(StorageFull)`, and every caller reports that as FL-4's `centralCopyFull`, never as a migration problem.
     - Any other failure answers `CopyResult::Unavailable(CopyUnavailableReason::MigrationPending{kind, version})`. The same type carries `Uninitialized` and `Damaged{detail}` (PD2 E26).
     - Recording the failure is best effort, and a failure to record never replaces the original answer.
  3. **Normal handling,** such as a fence or a stale epoch.
- **The carrier.** Each caller maps `Unavailable` once:
  - **Session face** (pieces, reads, copy status): under the request's frame ID, `StorageFull` is `SessionRefusal::CentralCopyFull`, and every other reason is `SessionRefusal::CopyUnavailable{project, reason}`. The Router keeps its unacknowledged pieces and sends them again later.
  - **Welcome:** `ProjectWelcome::CopyUnavailable{project, lease, reason}`, so only that project is contained.
  - **Registry steps** (Initialize, Fence, CopyStatus, Retire; review R31-2). `Unavailable` is transient, never a terminal refusal, and never triggers reservation cleanup.
    - **What's kept:** the transfer's or claim's persisted phase (`Fencing`, `Draining`, `Wiping`, or a pending initialization), its request or attempt ID, its chosen epoch, its receipts, and Fleet's `Active` claim reservation.
    - **The answer carries the identity the operation already has** (M3.12 finding):
      - `Unavailable{transfer, reason}` for `MoveResult`, `TakeoverResult`, the claim answer and the retirement answer, which have a transfer;
      - `Unavailable{project, reason}` for first creation (`ReserveFirstEpoch`, whose pending lease is keyed by project) and for the takeover preview, which run before any transfer exists.

      No ID is invented. The registry resumes that phase when the same request is repeated after repair.
    - **Wire shape** (M3.12 finding). A sum that carries both kinds, such as `SessionAnswer` (claim and first lease) or `AdminResult` (transfers and preview), has one tagged case: `Unavailable{subject: BlockedSubject::{Transfer{transfer}, Project{project}}, reason}`. That avoids two `unavailable` cases told apart only by optional IDs. `MoveResult` and `TakeoverResult` keep their transfer-only `Unavailable{transfer, reason}`.
    - **A replay proves only that the replayed step did nothing.** A fence that committed earlier, whose answer was lost, stays committed.
    - **A move's holder** keeps its draining obligation and resends the same `Release`.
    - **A first creation** stays pending, and never becomes `projectRejected`.
  - **Admin routes:** `Health` answers before the prelude, and `Migrate` answers `Failed{version, reason}`, both as above. Any other admin call answers HTTP 503 with the typed body `{kind: centralCopyFull}` for `StorageFull`, and `{kind: copyUnavailable, reason}` for every other reason. A **separate** transaction then records the failure in `_copy_migration_failure`, creating that table itself if it doesn't exist, so the record survives a failed first migration or adoption.
- This is the copy's own bookkeeping, never a project's schema; project schema changes still arrive as pieces (§4).

**The operations view** consists of owner-only admin routes (`OwnerEdgeEffect`, rank 0), each one hop unless stated:

| Route | Calls | Returns |
|---|---|---|
| `GET /admin/v1/copies?after&limit` | Fleet: `Page` | projects in ascending ID order after `after`; `limit` is at most 100 and the server enforces it. Each comes with its directory status, freshness and holder (§6) |
| `GET /admin/v1/copies/{id}/health` | Copy: `Health` | answered **before** the migration prelude, from the tolerant read and raw reads that need no current schema: the stored and supported bookkeeping versions, the recorded failure, initialized, archived, fence epoch and holder, acknowledged position, the last piece's time, the project schema version and fingerprint, staged parts, and storage bytes. It works while a migration is failing. **Storage bytes are a plain input** (M3.12 finding), like `now` in §2a.
<br>• **Two shapes.** The transport request stays the unit `CopyOpsCall::Health`, so no caller supplies a size. Only the target copy's own shell can measure it, so that shell builds the core input `Health{storage_bytes}` before the step: the Worker from `sql.databaseSize`, the native shell from its own file measurement.
<br>• The core assembles the one typed response, with no size port and no SQL sentinel |
| `POST /admin/v1/copies/{id}/repair` | Copy: `Repair` | re-runs the copy's alarm duties: the `DirectoryUpdate` resend and staged-part expiry. No replicated row changes |
| `POST /admin/v1/copies/migrate?after&limit` | Fleet: `Page`, then Copy: `Migrate` for each project | the **sweep**: one bounded page per request (`limit` at most 25, server-enforced), in ascending project ID. `Migrate` takes the tolerant read before the prelude (an adopted copy is `from: 1`) and answers `Migrated{from, to}`, `Current{version}` or `Failed{version, reason}`. The page returns every result and `next_after` (the page's last ID). A failed copy is reported and the sweep continues: re-running from any `after` repeats only idempotent migrations. **What the sweep guarantees** is coverage of every copy that existed when it started. A copy created or imported after that, whatever its ID (an import keeps its original, possibly lower, ID), is already current: its creating event ran the migration prelude. So the sweep never needs to reach it (V-M3 commission-4 finding). A removed copy is just absent. The CLI drives the pages |

**Delete is retirement, owned by the registry** (reviews F11, R6).

`POST /admin/v1/projects/{id}/retire{request_id, expect_fence, expect_acked}` → `RegistryAdminCall::Retire{request_id, project, expect_fence, expect_acked}`. It's recorded as a transfer of kind `Retire`, keyed by `request_id` like every transfer. A repeated request returns its stored outcome, and the same ID with different content is `conflictingRequest`. It serializes with every lease, claim and holder change. Its steps:
1. **`Checking`.** Refused unless all of these hold:
   - the project has no lease in any state (an expired lease is still a lease, FL-9);
   - no transfer of any kind is in progress, a claim included;
   - the board is archived: the registry asks Fleet for `ArchiveStatus{project}` (registry → fleet).

   While the `Retire` transfer is active, a claim answers `TransferInProgress`.
2. **`Wiping`.** Registry → copy `Retire{transfer_id, expect_fence, expect_acked}`.
   - If the fence or the acknowledged position differs, the copy refuses `PreviewStale`, and the transfer ends refused.
   - Otherwise, **in one transaction**, it writes a retire receipt `{transfer_id, fence, acked, retired_at}` into `_copy_state` (`retired_at` is the step's central time, written once and never re-stamped by a repeated `Retire`, `Health`, the sweep or an alarm; review R33-1), marks it `retired`, and deletes every project row and the rest of its bookkeeping.
   - A repeated `Retire` with the same `transfer_id` returns `Retired` from the receipt, without rechecking the deleted position. Any other call to a retired copy answers `Retired`.
3. **`Directory`.** Registry → fleet `MarkRetired{project}`, which is idempotent.
4. **`Retired`.** The registry removes any `Unheld` row and keeps a permanent tombstone, in one transaction.

Each step is the continuation pattern of §2a, so a crash resumes at the stored step. The project's ID is never reused, so a retired project can't come back as new.

**The closed sums gain `Retired`:**
- PD2's `SessionRefusal`, for pieces and claims;
- `ProjectCopyRead`;
- the import results (`ImportSeed` and `RecordUnheld`).

Spec 3 FL-18 and spec 2 E24 state the observable `retired` outcome and its precedence.

**Archive** stays FL-5's board archive, which leaves copies readable. Exporting a copy to object storage first is later (U67).

**Re-seed (G32)** rebuilds a damaged held copy. It needs a new path that uploads the current holder's snapshot, because the holder prunes its change capture at the acknowledged position. Whether it's in this release is the owner's decision (G32). Until then, `health` reports a damaged copy (a fingerprint mismatch or a failed migration) and nothing rebuilds it.

**Proof (V9, V9d):**
- a pre-versioned populated copy adopted as version 1;
- a failing migration with `health` still answering;
- the sweep over a dormant copy and a failing one, interrupted mid-page and resumed;
- directory rows added and removed between pages;
- an over-limit request refused;
- `retire` refused while a lease, an expired lease or a claim exists, and refused on a moved fence;
- a crash between each retire step;
- a repeated retire;
- after retirement, a piece, an import, a claim and a read each answering `retired`.
