# Staging seed map for real project copies

This is the verification input contract for the Implementer's staging-only seed Worker. It is not a migration implementation or permission to seed production. Lead decisions: `01a1c08d-f161-7aa0-b0b9-2b5ad1510255`, `01a1c098-04f5-7ab3-8e39-9a8d9a22444e`, `01a1c09e-3d99-7eb1-b23a-12138d412a2d`, and `01a1c09f-acaa-7331-86e7-e5d908aad353`.

The target being reconstructed is old central **2636806a588916347044829971cfacc003a1c937** and its locked fleet-sync **b9da4b5f230f81430fdf1140fbb634cf64decd8e**. Source references below use `O:` for that central commit and `F:` for that SDK commit. The local checked-out SDK used by the verification example is **0b2742975ca8b20270929b89f4959c1987bdb090**; its output must qualify against the old schema before seeding. A successful current-library open alone does not prove old-Router compatibility.

Source excerpts and full-source hashes are in `production-copy-route-evidence.json`. Read the pinned source, not a later working-tree file with the same path.

## 1. Allocation and inputs

Use the staging central name and the existing five object classes only. Fail closed on any production name, origin or allocation mismatch. Object placement is `O:crates/collaboration-central-cloudflare/src/object_location.rs:3–9`: Registry `LEASE_REGISTRY/registry`, Copy `PROJECT_COPIES/<database UUID>`, Fleet `FLEET/fleet`, plus per-installation Connections and Mailboxes. The seed Worker is verification tooling, then is replaced by the exact old central before the Router baseline begins.

For each current held project obtain these facts from authorized copies, never from a migration report:

| Symbol | Meaning and source                                                                                               |
| ------ | ---------------------------------------------------------------------------------------------------------------- |
| D      | Database/project UUID, validated retained origin and original registration                                       |
| M      | Current writer peer UUID from the retained registration; validate against file owner and the intended Mac        |
| N      | Existing writer lease/fence number, distinct from a replication position's epoch                                 |
| S      | Existing authoritative sender registration start from `ReplicationSender::copies()`                              |
| C      | That sender's receiver UUID; the staging authoritative receiver configuration must use C                         |
| A      | Sender's saved acknowledged position, before any staging Router opens B                                          |
| H      | Position of the public `WriterSnapshot`, pinned with its row image                                               |
| V, F   | Exact snapshot schema version and fingerprint                                                                    |
| T, E   | Explicit staging Unix-millisecond seed and expiry times; E >= T, long enough for baseline enrollment and renewal |
| I      | Fresh staging Registry incarnation, independent of N; coherent with staging Fleet's incarnation fence            |

All u64 values use lossless JSON and canonical decimal strings in TEXT columns. `SyncTime` is a transparent i64 Unix-millisecond number (`F:crates/runtime-primitives/src/lib.rs:25–28`), not an ISO string. UUIDs reused from files stay unchanged; newly generated allocation/row IDs are UUIDv7.

The numeric retained floor is **unobserved**: `read_window` and the stored `retained_after` accessor are private. The public snapshot position is H, and the seed baseline is **P=H**. Do not invent a lower bound or relabel head rows as an earlier A. Snapshot pages and their manifest must share D/P/schema exactly. Preserve complete pages, including full DDL and every replicated row, as private artifacts.

## 2. A/B extraction and copy evidence

The original debug-format prerequisite was superseded for collection by Lead01a1c0a8; the signed passive SDK reader has now opened all67 actual Sunbook backups with unchanged A/B hashes. The authorized production-source scope is only each Mac's `~/.codex-router/agent-communication/projects/`; no secrets, source writes, Router restart or SSH. Sunclaw remains blocked: automatic approval review rejected the cross-host copy instruction before delivery; Lead is obtaining direct owner approval. No resend or alternate route is authorized.

For each database, create **B** by SQLite online backup into a fresh mode0700 directory outside every repository. Source arguments are `-readonly -nofollow -batch -bail`; require successful exit and a single `ok` from `PRAGMA integrity_check`. Create A by backing up sealed B, not by taking a second live-source cut: A and B must represent the same logical cut. Use immutable file URIs only for sealed-backup integrity checks and B→A reads; require no B WAL/SHM. Never use immutable on live sources. Hash B, record source/destination file identity and sidecar hashes, and never open B through the writable library. A may be changed by public writable open and migrations; preserve its before/after hashes. B's hash must remain equal before/after extraction. B becomes mutable only when the authorized staging Router later opens it.

The verification example's ordinary mode validates retained origin and registration with `RetainedFile::open_read_only`. Its seed mode opens only A using `ReplicatedDatabase::open`, a local LeaseHolder keyed to the independently read stored N (provisional at1, seeding(N) above1) and the existing BoardProjectSchema. No central/session is needed for this open. The context only restores persisted receipts; it is never credited as a fresh grant or renewal. Use public `snapshot()` and `ReplicationSender::copies()`; never call replicate/add_copy, change sender state, issue private SQL or modify the SDK. No transport is started. Read-only B hashing does not prove that a process never opened B; that claim also rests on the driver's explicit path separation and retained invocation evidence.

Classify every file, including retained and rejected epochs. Do not treat a historical retained file as another current writer. Report per-project authority loss, pending/refused/failed registration, missing authoritative receiver, mismatching receiver/holder, `needs_snapshot`, invalid positions, unsupported schema and exact extraction errors to Lead before seeding. Count current held projects with A < H separately from A = H. The tail count is unknown until writable extraction. Current passive observations cover67 files (four original lease1,63 original lease2; one holder), not yet67 qualified staging writers.

## 3. Enrollment and local identity

Use only the existing identity values in fresh debug roots: service identity JSON `{ "version": 1, "serviceId": <existing installation> }` and peer JSON `{ "version": 1, "peerId": M }`. Sources: `O:crates/collaboration-runtime/src/service_identity_storage.rs:9–20,53–67,120–135,169–184`. Keep these under the debug root's collaboration directory; do not copy production secrets or enrollment records.

Enroll through the real existing enrollment route with a fresh staging credential and staging Access material. `O:crates/codex-router-cli/src/fleet_command/enrollment_prepare.rs:84–110` takes those identities and generates a new credential; `O:crates/collaboration-central-core/src/machine_enrollment.rs:32–41,229–298,374–395` accepts caller-selected identities subject to conflict validation. `enrollment_publish.rs:68–88` persists the staging result. Do not seed machine_enrollments directly, invent credential hashes, or claim an enrollment fixture proves authentication.

The old Router then opens B and renews existing M,N. Ordinary unknown registration/import cannot restore arbitrary N: `F:crates/fleet-sync-lease/src/registration_rules.rs:130–149,218–275` and `import_rules.rs:107–123,174–181` allocate epoch1. A claim downloads/installs a new grant (`O:crates/collaboration-runtime/src/project_claims.rs:28–74`, `project_installation.rs:28–67,107–126`); it is not the baseline attach route. Do not claim or re-register to conceal a failed seed.

## 4. Registry minimum per project

Initialize verbatim old Registry DDL and migration ledgers through the pinned generated schema source. SDK lease versions1–4 and central-registry versions1–4 must have their actual old checksums; core journal tables are initialized empty. Do not add new migration-record tables before PD7.

The following are the minimum rows for a fresh staging **registered, held** project. JSON field casing is literal. Replace symbols with losslessly encoded values.

| Table and columns                                  | Value                                                                                                                                                       |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `_fleet_sync_databases(database,record)`           | D; `{ "database": D, "lifecycle": { "state": "registered", "origin": O, "importedFrom": null }, "revision": { "epoch": N, "change": 1 }, "changed_at": T }` |
| `_fleet_sync_leases(database,record)`              | D; `{ "database": D, "holder": M, "epoch": N, "expires_at": E, "last_renewed_at": T }`                                                                      |
| `_fleet_sync_epoch_floor(database,epoch)`          | D; canonical decimal N                                                                                                                                      |
| `_fleet_sync_epoch_history(database,epoch,record)` | D; canonical decimal N; `{ "database": D, "epoch": N, "holder": M, "granted_at": T, "ended_at": null }`                                                     |

Here **O** is a staging lifecycle origin, not the source-prefix notation: a source-backed original origin if independently available, otherwise explicitly synthetic `{ "kind": "empty" }` for the fresh staging fixture. `Origin` admits only `empty` or `seeded` with `value.position` (`F:crates/fleet-sync-protocol/src/lifecycle_message.rs:192–201`). A native file's Bootstrap kind is not sufficient to reconstruct the historical Registry import provenance. Do not use `{kind:"created"}` or fabricate an import digest.

Sources: `F:crates/fleet-sync-lease/migrations/0001_registry_rules.sql`; `registry_records.rs:31–69,110–127`; `registry_snapshot.rs:11–17`; `registry_storage.rs:124–139`. Registered lifecycle requires a grant and its epoch equals revision.epoch. Current epoch history must include database. `renew_rules.rs:84–99` renews matching M,N without allocating a new number.

This is a **new staging lineage**, not an export of production Registry history: only current N is inferred. Earlier epochs, reserved floors above N, revision change counters and import provenance are not recoverable from a single native file. Do not fabricate holder-change/transfer receipts. If the test claims those original Registry facts, it needs separate authorized source evidence; this minimal seed does not cover them.

Keep renewal attempts, transfers (including phases/history/admissions/cancels), takeover previews, lifecycle attempts, cleanup failures and schema-support rows empty in this fixture. Real Session/schema negotiation and renewals populate their normal rows. Empty transient tables do not prove production in-flight workflow preservation; those are separate migration fixtures.

Seed `registry_lease_incarnation(row_id,name,incarnation)` with fresh UUIDv7/name `registry`/canonical decimal I, and staging Fleet `lease_incarnation_fence(fence_id,name,incarnation)` coherently. The incarnation acquisition takes `max(previous,at_least)+1` (`O:crates/collaboration-central-core/src/fleet_lease_step.rs:44–58`); it is **not** writer N. `registry_lease_acknowledgements` can start empty and be established by ordinary renewal/publication. DDL: old registry migrations0002/0003 and core migration0005. Record actual resulting incarnation rather than asserting an unchanged fresh seed value.

## 5. Copy minimum per project

Initialize verbatim `F:crates/fleet-sync-receiver/migrations/0001_copy_rules.sql`,0002,0003 plus old core Copy hook and journal DDL. Use the generated production source, not handwritten lookalike tables.

Publish the complete head snapshot atomically before exposing Following. It must contain only replicated project schema and rows, not native writer/capture tables, local Router tables, credentials, or SDK sender bookkeeping. Apply snapshot `schemaSteps` in order, bind each row according to its full table column layout, preserve lossless integer/float/blob/null values, and validate foreign keys. Do not parse DDL by splitting on every semicolon; use the existing migration statement parser/source fixture mechanism appropriate to the actual statements.

| Table and columns                               | Value                                                                                                                                                                            |
| ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Replicated project tables                       | Complete row image at P=H, with the exact snapshot DDL, indexes and views                                                                                                        |
| `_fleet_sync_copy(singleton,record)`            | 1; `{ "state": "following", "copy": { "database": D, "role": "authoritative", "fence": N, "holder": M, "start": S, "acked": H, "schema": { "version": V, "fingerprint": F } } }` |
| `_fleet_sync_copy_tables(name)`                 | One lowercase replicated membership name for each member table/view; derive from the schema statements, not just nonempty row pages                                              |
| `_fleet_sync_schema_steps(version,record)`      | canonical decimal V; `{ "version": V, "statements": <complete snapshot schemaSteps> }`                                                                                           |
| `_fleet_sync_hook_migrations(version,checksum)` | Exact old core Copy hook migrations/checksums, including `_copy_summary` initialization                                                                                          |

Preserve S from the original registered authoritative receiver; do not reset start to H. Required invariant: S <= H and N >= H.epoch. Fence N may exceed the last position's epoch when no new commit has occurred after a transfer. `F:crates/fleet-sync-receiver/src/copy_contract.rs:5–14` validates these conditions. The JSON key is **fingerprint**, not digest.

The old seed-publication path is the behavioral template: `seed_publication.rs:30–57` checks common headers and schema fingerprint;71–82 replaces all rows;99–134 recomputes per-table digests;136–158 validates foreign keys and records the schema step. `schema_membership.rs:53–68` and `schema_reads.rs:124–146` define membership and fingerprint scope. Use the public snapshot manifest's table names/counts/digests to validate every table. A schema fingerprint alone proves no row contents.

Initialize `_copy_summary` through the old hook DDL and leave its contents empty until ordinary consumer writes publish a summary, unless a genuine matching source summary is separately supplied. The old Copy hook accepts no summary (`O:crates/collaboration-central-core/src/copy_receiver_rules.rs:22,98–108`); do not invent a project summary from a single dummy task row.

For this quiescent Following fixture, `_fleet_sync_receipts`, `_fleet_sync_staged_parts`, `_fleet_sync_fence_receipts`, `_fleet_sync_seed`, `_fleet_sync_seed_pages`, `_fleet_sync_seed_attempts`, `_fleet_sync_install`, `_fleet_sync_install_pages` start empty. Their production histories are not reconstructed. `_fleet_sync_creation` also starts empty for this minimum existing-registration/ordinary-sync route: duplicate Create is **not covered**, because `copy_lifecycle.rs:32–51` requires the genuine original request. Do not fabricate a Create request and call it receipt continuity. A Create call during baseline is a failure to investigate, not permission to reset the Copy.

## 6. Sender and receipt continuity when A < H

The native B file and its sender registration/status are never rewritten by the seed Worker. The new staging Copy has no old piece receipts. When an authorized piece from A reaches a Following Copy already at H:

1. Receiver checks holder/fence before receipts (`F:crates/fleet-sync-receiver/src/apply_rules.rs:48–69`).
2. With no matching receipt and `piece.through <= H` or `continues_from != H`, it refuses with **Gap { acked: H }** (`apply_rules.rs:74–86`). It does not dedupe as AlreadyApplied.
3. Sender accepts H only within `[status.start,captureHead]`, saves ACK=H and drains if changed and not needs_snapshot (`F:crates/fleet-sync-writer/src/replication_sender.rs:376–395`). This is ordinary acknowledgement reconciliation; it is not an application-data write or snapshot install.
4. At A=H no old piece is needed; observe a later ordinary write and its ACK to prove the route.

This works for a **healthy** authoritative sender. `needs_snapshot=true` prevents an offer (`replication_sender.rs:233–235`); an authoritative ACK below retained floor fails with `authoritative.capture_floor` (`sender_retention.rs:50–54`), and invalid continuation fails `capture.continuation` (`capture_stream.rs:339–340`). `replicate_pass` enforces retention before preparing/sending (`replication_sender.rs:271–294`). The extractor reads status only and must not simulate acknowledgement by calling replicate_once with a fabricated Gap response. Actual old-Router ordinary traffic must close the healthy-tail observation.

Thus there is **sender continuity**, while original remote receipt history is intentionally absent in the new staging seed. Report this limit; no successful migration claim about historical receipts follows from this setup. Report unhealthy real files to Lead first, before seeding; no normalization or forced repair of the copied sender.

## 7. Required pre-PD7 baseline and later comparison

1. Keep the original B manifest, metadata sidecars, sealed hashes, A extraction and original public content digest. Qualify each signed old0.1.125/new0.1.135 artifact separately.
2. Seed the allocated staging objects, then replace the seed Worker with exact old central2636806a5. Enroll the isolated old Routers with reused IDs/fresh credentials. No production-root reuse.
3. Open the existing B copies. Require each M,N renewal to succeed, no claim/install/re-register, unchanged physical file identity/replacement record, and actual central Session authentication. Record all refusals.
4. Independently read old central Copy health and the complete project contents through the other Mac's non-holder public project view. Require D,role,N,M,S,H,V,F and all manifest/table/content digests to agree with the head image. Observe healthy A<H acknowledgement reconciliation before PD7; retain raw request/result evidence where supported, and report telemetry gaps rather than hand-authoring success.
5. Record complete Router workspace/board/project/file lists and staging Fleet directory facts. Directory payloads come from public existing metadata/approved fresh staging context; they are not inferable from Registry rows alone. Preserve scope distinctions: real project contents, fresh staging graph/enrollment/history.
6. Run PD7, compare independent before/after observations, upgrade only the isolated Routers to135, and observe the same enrolled identities/credentials and same-number renewals (S7), with ordinary bidirectional writes/readback. The seed Worker is not the comparison oracle.

Current evidence: Sunbook67/67 B/A copies;63 ordinary online backups and4 scratch-recovered pairs whose two collections matched on attempt1, with the repeated-hash atomicity limitation retained. All67 signed passive opens passed, A/B hashes unchanged. This is format/original-registration evidence only. Numeric floor, sender tail count, seeded baseline, hosted migration and upgrade remain unverified. Cargo qualification of the writable extractor waits the paired Implementer. The updated full procedure and private receipt paths are in s2-project-copy-plan.md.

## Opening-context qualification

The first provisional-only export passed four original-N1 files and rejected63 original-N2 files. The next seeding-only export passed those63 but rejected the four N1 files. These were apparatus-context failures, not observed stored authority loss. The corrected context follows project_store_registry's branch keyed to stored N. Both raw attempts and all sealed Bs are retained; a complete shared genesis fixture and the known2 sensitivity guard the two cases. Also, the63 N2 originals are physically named e1.sqlite; ordinary old125 opening without install/claim remains a separate baseline obligation. No renaming or normalization is allowed.
