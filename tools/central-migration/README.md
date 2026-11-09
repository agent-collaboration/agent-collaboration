# Central in-place migration Worker (one-time)

Design: `docs/specs/2026-11-09-d1-in-place-migration/` (PD1–PD7) in the design worktree. The governing column maps are copied to its `manifest/` folder. This folder is deleted after production (R11).

A TypeScript Worker, deployed under central's name with central's five SQLite storage classes, so it runs inside the existing objects. It answers four owner-authenticated calls and refuses everything else, including Routers' `/v1/session`, with 503:

| Call | What it does |
| --- | --- |
| `GET /admin/v1/migration/inventory` | The dry run. Every project with its holder, lease number, directory entry, copy state and action, every stop (R7), the run's counts, and a full dump of the three retiring classes. Writes nothing (`rowsWritten` 0). |
| `POST /admin/v1/migration/migrate` | Re-runs the inventory's checks. Any stop refuses the whole run before the first write. Otherwise it writes each copy, then the Fleet object, each in one storage transaction. A second run reports `alreadyMigrated` and writes nothing. |
| `GET /admin/v1/migration/verify` | Reads back every carried row and each copy's records and state, against the old sources still in place. |
| `POST /admin/v1/migration/revert` | Before PD7 step 5 only: drops main's tables and renames the `legacy_` tables back. Refused when nothing was migrated. |

The owner check mirrors central's rules (`src/owner_access.ts`, with file:line references to main).

## What migrate writes

- **Fleet object:**
  - renames the four tables main reuses, and the failure table, to `legacy_<name>`;
  - creates main's central-fleet, central-registry and fleet-sync-lease tables;
  - carries the graph, enrollments, the feed head and floor (both at the old head), and the larger of production's two lease incarnations;
  - copies every library lease table byte-exact at production's shape. Main's own `validate_legacy_rows` plus 0005/0006 convert them on first open;
  - records each set in its sqlx-standard table (`_sqlx_migrations_<set>`, PD1).
- **Each copy:** its failure table to main's shape, and its receiver records into `_sqlx_migrations_fleet_sync_receiver`. Project tables and the library's hook ledger are untouched.
- **Run time:** one recorded instant, in whole seconds, is used for every `created_at`, `updated_at` and `installed_on`.

## Commands

From this folder:

```sh
pnpm install --frozen-lockfile
pnpm run check && pnpm run lint && pnpm run format:check
pnpm test   # builds, then runs the tests in real workerd through Miniflare
node scripts/generate_schema_sources.ts   # after a schema source changes
```

- **Embedded SQL:** `src/generated/schema_sources.ts` holds production's DDL (git `2636806a5`, fleet-sync `b9da4b5`) and main's migration sets, byte-for-byte. A test fails when the file is stale.
- **How the tests run:** they seed production's exact schema through a test-only stand-in for the old central (`test/support/old_central_stand_in.ts`), under the same Worker name and persistence path, then run the migration Worker over that storage. No test waits on time.
- **Deploy configs:** `scripts/deploy_configs.ts` derives PD7's two configurations from the live target's own config:
  - step 2, the migration Worker;
  - step 5, main's central, with the three retired classes as `deleted` tombstones.
