# Independent in-place migration observations

Verification apparatus for S1/S2/S7. No staging migration or Router upgrade has run. Lead-authorized Sunbook project backup collection and passive retained-file observations have run (67/67); the backup tool reads only the explicit project tree and creates private copies. Other tools never deploy, migrate, enroll, change holder or start/restart a Router. Adopted design: `codex-router.design-central-storage-reset/docs/specs/2026-11-09-d1-in-place-migration/` (R1–R15, S1–S7). Later review corrections still govern the migration and the live target must be declared again.

The governing staging procedure is [staging-runbook.md](staging-runbook.md). Earlier setup plans supply source-qualified observation details; their deployment/allocation sequences are superseded. No staging command runs until the Lead releases that joint runbook.

Current private copy-baseline evidence is under `/private/tmp/2026-11-ledger-adoption-01a1c0e4-6ea1-76be-b49d-cb1f85a1bf3f/`:

- `proof-summary.json`: 67 local f0528317 SDK journal adoptions preserve original receipts, lease/sender/window and byte-identical public manifests (80,196 application rows); B hashes stay unchanged. Originals already have six ledger columns, so this does not cover an extended-ledger rebuild.
- `checksum-refusal/result.json`: one sacrificial checksum mutation, then explicit refusal with unchanged file-set hashes. No further checksum mutation belongs in a staging capture.
- `metadata-merge-summary.json` and `mac-files-70-f052.json`: 67 local plus 3 peer-executed Sunclaw facts, validated by the migration Worker's actual decoder. Merged SHA256 `e20c8ee3d80d47847e8798e872a4f3c4224d126ebe7d029a448816f09f970c0d`. Sunclaw's exact metadata reply is Router push `01a1c130-11a4-7043-ace1-e71a5ad3adf4`; remote rows/logs were not fetched. This body is an original-copy baseline, and must be refreshed from owned staging roots at the final migration cut.
- `production-directory-sunbook/production-directory.json`: independently captured public W/B/P lists, complete 4/16/70 via 21 read-only commands. `record-production-directory.ts` is the narrow authorized production collector; the staging collector continues to refuse production roots. Compare production→staging only on project ID/title → board → workspace, per Lead `01a1c11c`; the full intra-staging S1 graph oracle stays strict.

The recorder uses **existing** independent surfaces, not the migration Worker's inventory/verify response. It records every page of `GET /admin/v1/copies?limit=100[&after=UUID]` and each listed project's `GET /admin/v1/copies/{id}/health`, retaining private raw replies and SHA256. Empty inventory, repeated cursor/identity and mismatched health database fail. Requests are GET-only, same-origin, no redirects, bounded by a failure timeout. Existing handlers may initialize or maintain state internally; this is HTTP-read-only, not a promise of storage-level passivity.

`record-router-lists.ts` drives only public `workspace list`, archived-inclusive `board list`, and `project list` against a signed declared CLI and isolated service directory. It can also read each explicitly declared non-held project with complete workstream/task/post listings. It handles the actual tagged `Record` shape for workstream/task and direct Post shape, follows cursors, rejects unavailable pages and retains original CLI replies. Full public records are canonicalized by key order and record identity; integers remain exact. This is a digest of the selected public view, not a database-page/content hash. The non-holder routing must be qualified on the declared staging pair; a caller-supplied identity alone is not routing proof.

## Commands

Run from the migration worktree root with the pinned Node and pnpm versions. Install only the existing lockfile:

```sh
pnpm --dir verification install --frozen-lockfile
node verification/observations/central/inplace-migration/record-router-lists.ts --help
node verification/observations/central/inplace-migration/migration-observe.ts --help
node verification/observations/central/inplace-migration/record-router-lists.ts "$PRIVATE_ROUTER_LIST_PLAN"
node verification/observations/central/inplace-migration/migration-observe.ts record "$PRIVATE_CENTRAL_PLAN"
node verification/observations/central/inplace-migration/migration-observe.ts compare "$BEFORE_SNAPSHOT" "$AFTER_SNAPSHOT"
```

All plans/readbacks must be owner-private regular JSON files, mode0600, no symlinks. Evidence directories must be fresh mode0700 and canonical absolute paths; all writes use exclusive creation. Central plan:

```json
{
  "centralOrigin": "https://DECLARED-STAGING-ORIGIN/",
  "sourceHead": "FULL_DECLARED_40_HEX_SOURCE_HEAD",
  "phase": "before",
  "routerTarget": {
    "sourceHead": "FULL_DECLARED_ROUTER_COMMIT",
    "version": "DECLARED_WORKSPACE_VERSION"
  },
  "routerProjects": ["EXACT_SELECTED_PROJECT_UUID"],
  "evidenceDirectory": "/private/DECLARED/FRESH-CAPTURE",
  "ownerAssertionEnv": "MIGRATION_OWNER_ASSERTION",
  "routerObservationFiles": []
}
```

The assertion is supplied in memory by the existing secret-safe wrapper/`op run`, never in argv, plans or evidence. Before/after must use the same central origin and distinct run IDs. A central-only capture is useful but **cannot** pass full comparison. It remains incomplete without both Routers' observations.

Router list plan has `machineLabel`, signed `router`/`cli` artifacts (`path`, `sha256`, `identifier`, `team`), full `sourceHead`, actual `expectedVersion`, `serviceDirectory`, `evidenceDirectory`, and explicit `contentRequests`. Before uses the old125 tag; after uses the declared SDK-f052 bump commit and its actual workspace version. Each content request declares `project`, `readerInstallation` and `holderInstallation` (null only for an explicitly unheld project); record held projects from the non-holder. The CLI and reported service version are checked when present. No binary is built or installed by this collector. Production roots are refused. The staging collector also enumerates all workspace ADRs and preserves repository references already present in public workspace facts.

`host-status.ts` records the product's actual connected status text, peer, staging origin and exact instance/connection/revision counters. It does not independently assert renewal or enrollment continuity. `Installation.markUpgradeBaseline()` and `Installation.upgrade()` bind root reuse to the original identity/enrollment fingerprint and the append-only private command ledger; all commands against those roots must use the registered wrapper. `command-ledger.ts` refuses incomplete bounded-command outcomes and derives post-baseline `fleet enroll` counts. This proves only the controlled launcher's command coverage. Actual staging startup/upgrade and same-N renewal remain unobserved until the declared run.

## Router observation assembly and remaining adapter gap

`contracts.ts::routerObservation` is the typed independent-readback input to the central recorder. It contains machine/installation identity, version, all list facts and their sources, each local holder's `writers[{database,holder,lease}]`, non-holder `content` records, completeness flags, and after-migration S7 authentication/renewal evidence. `source` entries contain only artifact path, exact SHA256 and one of `routerPublicRead`, `retainedWriterRead`, `machineAuthentication`. CLI comparison checks every retained source file's mode/type/hash before comparing. Full source bodies remain private. Unknown/error/missing data never becomes an empty successful collection.

The list/content CLI capture supplies the list/content portion. The signed retained-writer example, snapshot exporter and scratch ledger inspector have passed six apparatus tests, Clippy and help at SDK `f05283174c980787101ebddff5776000decb2c26`. Its passive adapter guards the original D/M/N/origin and rejects known loss. Public snapshot export and journal initialization run only on fresh writable A; sealed B is hash-checked. The approved private SQL exception reads scratch ledger/window facts and permitted exactly one sacrificial checksum mutation for the refusal proof above. `build-mac-files.ts` binds the passive/header/completion/inspection evidence and emits exact receiver/floor/head facts; head comes from the public writer snapshot, floor from the approved scratch projection. These reads do not observe a current renewal. The live S7 authentication/renewal adapter and fresh staging cuts remain execution prerequisites. Lead01a1c07d forbids commits of the example; the seeded wrong-lease sibling test stays in the working tree and is never committed. See `s2-project-copy-plan.md`.

`project-backups.ts` implements the authorized online B/A collection with immutable mode confined to sealed copies. `seed-snapshot.ts` retains the SDK manifest and checks complete framing, identity/fence and row counts; it does not independently recompute SDK table digests. The original public exporter reports floor unobserved. The later, separately approved scratch inspector supplies it; do not rewrite the public exporter result as though that API exposed floor. Normal SDK opening never touches live or sealed B files.

The owner reduced Router baseline/upgrade membership to four Sunbook and three Sunclaw copies; the complete central cohort remains 70. Capture plans carry an explicit `routerProjects` list, identical before/after. Only selected projects require source-bound native writer/content/renewal evidence; central Registry/Copy/directory comparisons remain complete for all central projects. The private `selected-router-projects.json` retains picks and counts from already exported SDK rows. Missing selected evidence, either Mac's identity, complete Fleet lists or actual S7 authentication remains incomplete. Results are `changed` (exit1), `incomplete` (exit2), or `preserved` (exit0) for the covered observations; capture/shape failures exit3. A preserved fixture is not migration acceptance.

Protected comparisons:

- R1: per-Mac database/holder/lease; after S7 renewal must match the stored writer exactly, including integers above JS safe range.
- R3: each project's copy database/role/fence/holder/start/acked/schema, stage and `projectFingerprint`, plus canonical non-holder public content digest. `projectFingerprint` is **schema-only** (`copy_inspection.rs` → `SchemaReads::fingerprint` → schema entries). It is never called row-content proof.
- R5: all directory IDs/names/workspace/board/archive state; each Router's normalized workspace, board and project-directory facts. Original envelopes/transport observation times remain in raw evidence, not in stable Fleet metadata comparisons.
- R6/S7: same machine/installation IDs and actual after-migration authentication with existing credentials, no enrollment and same-number renewal. The recorder does not claim to enumerate unseen enrollment rows.

Accepted remaining gap (Lead01a1c068): registry-only/orphan rows have no independent read surface; migration inventory, verification-only Rust decoder and S4 negative cases are a separate second line. Physical Durable Object namespace identity is separately pinned by the deployment operator's inventory; matching project IDs/class names alone is insufficient. Project row content outside the selected public view is not independently hashed by this tool.

Keep the before/after cuts quiescent and capture content before performing S1's new cross-Mac writes. There is no atomic snapshot spanning HTTP pages and Mac reads. Renewal expiry timestamps/storage sizes/bookkeeping versions can change normally and are not preserved-identity assertions. Do not compare new post-migration application writes to the earlier fixed content digest.

## Source anchors and qualification

At old `2636806a588916347044829971cfacc003a1c937` and current baseline `b2a0d22809580048102e3725736a5a554855672c`: `central_router.rs:35–44` and `owner_http_routes.rs` expose copy directory/health. `copy_health.rs` joins library CopyInspection; library `fleet-sync-receiver/src/copy_inspection.rs:138` and `schema_reads.rs:145` establish the fingerprint's scope. `fleet-admission/src/directory.rs` defines independent directory sides. `message-board/src/board_metadata.rs:134–162` and `board_pagination.rs:60` define Router Fleet list records; `project_feed.rs:32–60` defines tagged entity records. Actual on-staging shape/version qualification is still required after reviews and deployment availability.

Tests here verify recorder/comparer refusal and real local HTTP transport, not migration behavior. Run package `check`, `lint`, `format:check`, `test`; no real-time outcome wait is used. Fixture timeout only bounds the HTTP/CLI wait. No production folder or staging write is needed for Phase0.
