# Central in-place migration Worker (one-time)

Design: `docs/specs/2026-11-09-d1-in-place-migration/` (PD1–PD7) in the design worktree. The governing column maps are copied to its `manifest/` folder. This folder is deleted after production (R11).

A TypeScript Worker, deployed under central's name with central's five SQLite storage classes, so it runs inside the existing objects. It answers four owner-authenticated calls and refuses everything else, including Routers' `/v1/session`, with 503:

| Call | What it does |
| --- | --- |
| `GET /admin/v1/migration/inventory` | Central's side of the dry run: every project with its holder, lease number, directory entry and Copy facts (state, role, fence, holder, start, acknowledged position, schema), the configured authoritative receiver, every stop, the counts, and a full dump of the three retiring classes. Writes nothing (`rowsWritten` 0). |
| `POST /admin/v1/migration/inventory` | The same, with migrate's body: migrate's whole decision as a dry run. |
| `POST /admin/v1/migration/migrate` | Body: S2's facts from each holder Mac (below). Re-runs every check; any stop refuses the whole run before the first write. Otherwise it writes each copy, then the Fleet object, each in one storage transaction. A run interrupted between objects resumes on the next call, and a finished run reports `alreadyMigrated` and writes nothing. |
| `GET /admin/v1/migration/verify` | Reads back every carried row and each copy's records and state, against the old sources still in place. Each object's records are checked at the time that object committed; an interrupted run's stamps need not be in order. |
| `POST /admin/v1/migration/revert` | Before PD7 step 5 only: every copy, then the Fleet (whose marker goes last), drops main's tables and renames the `legacy_` tables back. An interrupted revert, or one after a migrate that stopped before the Fleet, finishes on the next call. Refused when nothing was migrated; migrate refuses while a revert is unfinished. |

### Migrate's body: the holder Macs' retained files (S2)

```json
{ "macFiles": [ { "database": "<project UUIDv7>", "holder": "<writer UUIDv7>", "lease": 3,
  "senderReceiver": "<receiver UUIDv7>",
  "retained": { "floor": { "epoch": 1, "sequence": 0 }, "head": { "epoch": 3, "sequence": 20 } } } ] }
```

One entry per held project, read from the Mac's sealed backup without opening a writer. Migrate stops when an entry is missing or names an unheld project, when the file's writer or lease differs from the registry's, when its sender's receiver differs from central's configured `authoritativeReceiver` (CENTRAL_REGISTRY_CONFIG, deployed with this Worker), or when the Copy's acknowledged position is outside `floor`..`head`.

### What is refused before any write (R7)

Besides the shape, history, journal, holder, directory and Copy contradictions, every carried value is decoded the way main's first open decodes it: each library lease table row as fleet-sync-lease `validate_legacy_rows` reads it (`src/lease_records.ts`), each Copy record as the receiver's `CopyRecord::validate`, each enrollment as `StoredMachine::decode` (`src/enrollment_record.ts`), and both incarnation counters as canonical decimals within i64. Integers stay exact end to end (`src/exact_json.ts`): a value beyond 2^53 is read as its decimal text and bound back through `CAST(? AS INTEGER)`.

The owner check mirrors central's rules (`src/owner_access.ts`, with file:line references to main).

## What migrate writes

- **Fleet object:**
  - renames the four tables main reuses, and the failure table, to `legacy_<name>`;
  - creates main's central-fleet, central-registry and fleet-sync-lease tables;
  - carries the graph, enrollments, the feed head and floor (both at the old head), and the larger of production's two lease incarnations;
  - copies every library lease table byte-exact at production's shape. Main's own `validate_legacy_rows` plus 0005/0006 convert them on first open;
  - records each set in its sqlx-standard table (`_sqlx_migrations_<set>`, PD1).
- **Each copy:** its failure table to main's shape, and its receiver records into `_sqlx_migrations_fleet_sync_receiver`. Project tables and the library's hook ledger are untouched.
- **The journal, Fleet and each copy:** when main's pinned library records journal receipts in `_fleet_sync_migrations`, production's `_fleet_sync_schema` journal (marker 1) is converted in the object's own transaction. The original tables are copied to `legacy_journal_*` for revert. Main's later journal SQL runs for real, and every corpus migration gets the library's receipt. With a library that still keeps the marker, the journal is left alone (main adopts it). The hook ledger is never touched: receiver v4 and the library's `initialize_hook` adopt it on main's first open, so only production's receiver versions (1–3) are recorded.
- **Run time:** one recorded instant, in whole seconds, is used for every `created_at`, `updated_at` and `installed_on`.

## After main's deploy (PD7 step 5)

Main reads a Copy's health without migrating it. When main's receiver corpus is newer than production's (agent-fleet-sync's ledger fix adds receiver v4), each Copy reads `unreadable` until it applies the pending migration. Run main's owner Copy migration, `POST /admin/v1/copies/migrate?limit=25`, paging with `after` until `nextAfter` is null. That covers every Copy in the Fleet directory. A retired project has no directory entry, so run `POST /admin/v1/copies/{id}/repair` for each retired Copy; it answers 410 `retired` once current. `test/main_central` does both.

`scripts/run_copy_migration.ts` runs both steps and then reads every Copy's health. It takes the migration Worker's saved inventory, `--receiver-version <n>` and `CENTRAL_OWNER_ACCESS_TOKEN`. Any result that isn't `migrated` or `current`, a repair that isn't 410 `retired`, or a live Copy not at `<n>` stops it with exit 1. Otherwise it prints one line per Copy (state and receiver version) and `{"outcome":"complete"}`.

## Commands

From this folder:

```sh
pnpm install --frozen-lockfile
pnpm run check && pnpm run lint && pnpm run format:check
pnpm test   # builds, then runs the tests in real workerd through Miniflare
# Main's own central over the migrated storage (needs this checkout's Worker build):
worker-build ../../crates/collaboration-central-cloudflare --profile worker-release --no-opt -- --locked --no-default-features
MAIN_CENTRAL_BUILD="$PWD/../../crates/collaboration-central-cloudflare/build" pnpm run test:main-central
node scripts/generate_schema_sources.ts   # after a schema source changes
# A local proof against an unpinned library (never commit what this generates):
CENTRAL_MIGRATION_LIBRARY_MAIN_DIR=<agent-fleet-sync checkout> node scripts/generate_schema_sources.ts
```

- **Embedded SQL:** `src/generated/schema_sources.ts` holds production's DDL (git `2636806a5`, fleet-sync `b9da4b5`) and main's migration sets, byte-for-byte. A test fails when the file is stale.
- **How the tests run:** they seed production's exact schema through a test-only stand-in for the old central (`test/support/old_central_stand_in.ts`), under the same Worker name and persistence path, then run the migration Worker over that storage. Interrupted runs use a test-only entry (`test/support/interrupted_migration_worker.ts`) that runs the real migrate and revert with a fixed run time and stops after a chosen number of object writes. No test waits on time.
- **Deploy configs:** `scripts/deploy_configs.ts` derives PD7's two configurations from the live target's own config:
  - step 2, the migration Worker;
  - step 5, main's central, with the three retired classes as `deleted` tombstones.
