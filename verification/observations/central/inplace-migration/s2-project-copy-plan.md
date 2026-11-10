# S2: full staging rehearsal using real project copies

The owner requires staging testing with real production project data. Lead01a1c086/01a1c08d replaced the original read-only reopen plan with a full old-central baseline, PD7 migration and isolated Router125→135 upgrade. The complete seed input contract is [staging-seed-worker-spec.md](staging-seed-worker-spec.md); its old-central2636806a5/fleet-syncb9da4b5 pins govern reconstruction. Fresh staging graph/enrollment/history must never be described as a copy of production central state.

## Authorized collection

Lead01a1c09f authorizes SQLite online backup of every database under each Mac's `~/.codex-router/agent-communication/projects/` into a new private directory outside repositories. No production lifecycle operation, secrets or SSH. Lead01a1c0a8 explicitly releases copy collection before Cargo-backed compatibility qualification and binds immutable reads of sealed B. Our initial Sunclaw instruction was rejected by automatic approval review: cross-host authority/destination trust were not sufficiently established. Nothing was delivered by that call. Lead subsequently reports a direct owner response and completed Sunclaw collection through Station Chief; its three copies and manifest are peer-reported, not independently read here. Lead01a1c0c0 authorizes a distinct local Sunclaw source build plus shared-fixture comparison, with no SSH/upload or binary transport.

`project-backups.ts` enumerates canonical UUID project directories and canonical positive-epoch SQLite filenames, including rejected epochs. It rejects unknown files, escaping aliases and symlinks. WAL/SHM are runtime files; they are incorporated by online backup and are never copied beside a sealed B. Unknown sync metadata is a gap to resolve, not a silently omitted sidecar.

For an ordinary source, use argv arrays:

```text
sqlite3 -readonly -nofollow -batch -bail SOURCE ".backup 'B'"
sqlite3 -readonly -nofollow -batch -bail 'file:B?immutable=1' 'PRAGMA integrity_check;'
chmod 0400 B
sqlite3 -readonly -nofollow -batch -bail 'file:B?immutable=1' ".backup 'A'"
sqlite3 -readonly -nofollow -batch -bail 'file:A?immutable=1' 'PRAGMA integrity_check;'
chmod 0600 A
```

The actual driver uses encoded absolute file URIs. B→A uses B as its source so both represent one logical cut. Immutable is permitted only for completed quiescent copies: **never on a live production file**, where ignoring WAL would omit committed contents. Require successful backup/integrity exits, exactly `ok`, no B WAL/SHM, private modes, full hashes and source file identities. Stock SQLite version observed3.51.0. A first plain read-only open of a WAL-mode backup without SHM returned CANTOPEN14; immutable checks passed on the controlled SQLite fixture.

Current Sunbook receipt: `/private/tmp/2026-11-central-project-copies-01a1c0a9-d9b7-7951-bead-f64aa22da62d/manifest.json`. Initial collection discovered67 databases,63 successful B/A pairs and4 source-open failures. The63 Bs total158253056 bytes. `sealed-check.json` independently confirms all63 hashes unchanged, mode0400 and no runtime sidecars. Four failures have nonempty WAL/noSHM. Read-only lsof finds the live process's handles but does not show an exclusive-lock flag. See `source-refusal-diagnosis.json`; no corruption claim follows.

The fallback now produced4/4 copies on attempt1, so67/67 copies are available. Its receipt is the same root plus `recovery-01a1c0ae-a097-79d1-b6fd-f3af522319a4/recovery-manifest.json`. The signed passive reader then opened all67 A files, with no refusals and unchanged A/B hashes (`retained-observations.json`); original leases were4 at1 and63 at2, one holder. This is no current-renewal claim. Lead01a1c0ac authorizes the capped fallback used for the four: collect DB+WAL twice into separate scratch directories; compare hashes; if identical, recover only scratch with SQLite, then online-backup B/A and immutable integrity. Retry changed collections up to5 times; exclusions stay explicit. Equal repeated hashes are evidence of repeated identical collections, not a logical guarantee of atomicity or no ABA writes. Keep this limitation with the later SDK/public-content evidence. Never create SHM/checkpoint on the production source or stop the Router.

## Public A/B extraction

The verification-only example is `crates/board-staging-cutover/examples/retained_writer_observation/`. Ordinary mode is the existing passive retained read; it checks project/origin/original registration/known authority loss and never claims current renewal. Seed mode opens only disposable A through public `ReplicatedDatabase::open`, using a local LeaseHolder keyed to independently read original N (provisional at1, seeding(N) above1), BoardProjectSchema and no network transport. Public `WriterSnapshot` supplies a head-pinned row/schema image at H; `ReplicationSender::copies()` supplies receiver/role/start/acked/needs_snapshot. Numeric retained floor has no public accessor and remains unobserved. No private SQL or SDK/runtime edits.

Seed mode outputs JSON Lines. Header/pages/manifest are incomplete unless the terminal `seedSnapshotComplete` record exists and the process succeeds. Retain raw private output; require B hashes before/after equal, distinct file identities for A/B, signed artifact hash/identity, and matching D/M/N/schema/positions. A may change during writable open/migrations, so preserve its before/after hashes and qualify the schema against the old target. Do not claim current-library migration output was original B content without that qualification. Discard A only after its evidence is kept and no process uses it; keep B for the actual staging Router.

Classify every file and project: current held, retained, registration failure, authority loss, absent receiver, wrong receiver, needs_snapshot, continuation error, unsupported schema or other exact failure. Count current authoritative A<H tails and A=H separately. No tail count exists until extraction runs. An unhealthy source is reported to Lead before seed; no sender repair/normalization.

## Old baseline, migration and upgrade

1. Build the source-reviewed staging-only seed Worker from the seed map; allocate only existing staging classes and reject production. Seed a fresh Registry current-N lineage and a complete Copy image at P=H, with original S/receiver and schema. Enroll reused identities through the real route with fresh staging credentials. Never seed machine credentials directly.
2. Replace the seed Worker with exact old central2636806a5. Start only isolated, signed Router125 artifacts against staging and the B copies. Require same-number M,N renewal, real Session authentication, no claim/install/re-register and unchanged physical file identity/replacement evidence. Native file hashes can change through ordinary writes; hash equality is not the no-install proof.
3. For healthy A<H, observe the ordinary old sender's reconciliation: receiver lacks old receipts and returns Gap(H); sender validates and saves H. Already-needs-snapshot/below-floor senders do not get counted as passing. Empty staging receipt tables do not prove original production receipt preservation.
4. Before PD7, independently verify every Copy's D/role/fence/holder/start/acked/schema and complete head content. Read each project through the other Mac's non-holder public view, retaining canonical records and raw replies; compare full snapshot manifest/table digests where the qualified read surface supports them. Schema fingerprints alone prove no rows. Complete Fleet workspace/board/project lists too.
5. Run PD7 under the Lead's separate deployment release. The migration Worker report is not the independent oracle. Capture the same independent surfaces before/after; keep quiescent cuts, then run new bidirectional writes/readback.
6. Upgrade only isolated Routers to signed135. S7 requires same identities and existing staging credentials, no new enrollment, actual authenticated Session and same-number renewals. Installed production Routers remain running and untouched.

Current limits: Cargo is owned by the paired Implementer's serial gates; the writable A/B extension and controlled debug qualification case are authored but not yet built. The frozen passive artifact has separately qualified actual backed-up Turso reads on67 files. Live S7 observation assembly, qualified non-holder route, seed deployment, actual staging baseline/PD7 and135 upgrade remain unverified. No commit is authorized; the seeded wrong-lease sensitivity test must remain working-tree-only.
