# Staging runbook: S1/S2 for the central in-place migration

**Execution remains gated.** The 🦁 Lead reports all 70 Copies seeded on staging. The 🔬 Verifier's local synthetic isolation probes passed, including the agent-authored variant; no real-copy baseline or migration observation has run here. The reduced 4+3 external baseline awaits direct user approval after an automatic-review denial. S4 Access is now ready according to Lead 01a1c20c: the Advisor created the owner-only app under direct owner authorization and readback passed. This Verifier's earlier create denial remains historical and was not retried. Owners: the 🐒 Implementer owns setup, seed, import, deploy and PD7; the Verifier owns marked observations. This joint runbook governs sequencing; earlier plans supply source-qualified observer details. Production processes are never replaced. Separately authorized production reads remain backup collection and public directory lists.

Conventions:

- **Private settings, never in this repository.** One owner-private JSON file, `$S1_SETTINGS` (mode 0600, outside every repository), holds the non-secret allocation: account, Worker name `collaboration-central-s1-migration`, hostname, Access team domain and audiences, owner identity, central configuration (receiver C), agent-vault mapping, and the debug roots and ports. The proposed file is `/private/tmp/2026-11-s1-staging/s1-settings.json`, awaiting the Lead's check. It's the input of every deploy below (`DeploySettings` in `tools/central-migration/scripts/worker_deploy.ts`). This runbook names keys, never values.
- **Credentials.** Cloudflare deploys use the cf CLI's own owner OAuth login, which is active on Sunbook, as fleet-ops requires (no API deploy token). The deploy wrapper refuses any cf token environment. Access service-token values come from the dev-agents vault through agent-vault and are never printed: `agent-vault vaults --json` names the saved mapping, and every command needing a token runs as `agent-vault --credential "$CREDENTIAL" op run --environment "$ENVIRONMENT" -- <command>`, with the token read inside the process (`CENTRAL_SEED_ACCESS_TOKEN`, `CENTRAL_OWNER_ACCESS_TOKEN`). `cf` runs from an owner-private scratch directory, with `umask 077`, shell tracing off and `WRANGLER_LOG_PATH` in that scratch.
- **Every step has a stop.** A stop means: do nothing further, keep the private evidence, and bring the exact output to the Lead. An unknown outcome (no answer, a timeout) is reconciled by reading state, never by repeating a write.
- **Status of each command.** `exists` = in a commit now; `to build` = a named gap, listed in §10, that must land and be reviewed before its step. No Python anywhere: every new tool is TypeScript.
- **Every deploy is dry-run first.** Each deploy command below runs once with `--dry-run`. That renders the config, runs cf's own `deploy --dry-run` (no upload) and prints the planned namespace diff (create, keep by id, delete). The real run follows only when that diff is the step's plan.

## 0. Allocation (decision for the Lead before anything else)

The seed Worker writes only into **fresh** storage: it refuses any object that already has tables (`verification/central-seed-worker/src/storage_objects.ts`). The existing one-core staging central (`fleet-ops/central/staging/one_core/normal`) already holds its Registry and Fleet objects, so seeding it would stop at the first object. The allocation this runbook assumes:

- a **new staging Worker name** with **five fresh Durable Object namespaces** for ProjectCopyObject, LeaseRegistryObject, FleetObject, ConnectionObject and MailboxObject, created by its first upload, as fleet-ops' C3 rehearsal did; never renaming, deleting or adopting an existing namespace;
- a dedicated staging hostname and staging Access applications (staging admin identity only);
- staging's old-central `CENTRAL_REGISTRY_CONFIG.authoritativeReceiver` = **C** `01a16eaa-ae36-7eec-9698-e8c34d10238b` (`staging-s1-setup-plan.md`, Staging allocation).

**Stop** if the Lead allocates an existing staging Worker instead: the seed's fresh-storage rule then needs a redesign, not a workaround.

Access, following fleet-ops' C3 pattern with the cf CLI, created only after the Lead checks the settings:

- the whole-host owner Access application (staging admin identity only);
- the `/v1/session` Access application;
- two debug-Router service tokens, one per Mac, stored in one dev-agents item through agent-vault and never printed.

Their audiences then replace the settings' `TO_CREATE` entries. **Stop** on any `TO_CREATE` or `TO_FILL` left in `$S1_SETTINGS` before §2.

## 1. Build the exact old central and main's central (exists)

Old central from a fresh extract of `2636806a588916347044829971cfacc003a1c937`, main from the reviewed head (re-declared after review; never a mutable worktree):

```sh
git archive --format=tar --output "$PRIVATE/old.tar" 2636806a588916347044829971cfacc003a1c937   # then extract into a fresh owned directory
worker-build crates/collaboration-central-cloudflare --profile worker-release --no-opt -- --locked --no-default-features
node verification/tooling/check-central-worker-bundle.ts production
shasum -a 256 crates/collaboration-central-cloudflare/build/index.js crates/collaboration-central-cloudflare/build/index_bg.wasm
```

Expected: worker-build 0.8.7, exit 0, the bundle check passes, and both hashes are recorded in the artifact's `bundle.json` (the shape fleet-ops' deploy guard requires). **Stop** on a lockfile change, a different worker-build version or a failed bundle check.

The seed Worker and the migration Worker build from this repository:

```sh
pnpm --dir verification/central-seed-worker install --frozen-lockfile && pnpm --dir verification/central-seed-worker run build
pnpm --dir tools/central-migration install --frozen-lockfile && pnpm --dir tools/central-migration run build
```

## 2. First upload: the seed Worker under the staging name

The first upload creates the five namespaces. It must be the seed Worker, not old central: any request to old central would bootstrap the Registry and Fleet objects and end their freshness.

- **Config** (exists): `seedWorkerConfig(staging, entrypoint)` in `verification/central-seed-worker/scripts/deploy_config.ts`. It refuses production's name and domain, keeps the five classes, and sets `STAGING_SEED_TARGET` to the Worker name. The name itself isn't a guard: the deploy only accepts the seed as a first upload (no existing Worker, namespaces or hostname), so it can never replace a live central.
- **Deploy** (exists, dd09aa820):

  ```sh
  node verification/central-seed-worker/scripts/deploy_seed_worker.ts --settings "$S1_SETTINGS" \
    --artifact verification/central-seed-worker/build --scratch "$PRIVATE/deploy-seed" [--dry-run]
  ```

  It refuses an existing Worker, existing namespaces for it, or an existing hostname. The dry run prints `"namespaces":{"create":[the five classes],"keep":[],"delete":[]}`.

Expected: one new version. After the deploy, exactly five namespaces bound to the new Worker, each newly created, and no other namespace changed. **Stop** on any namespace that existed before, any change to another Worker's namespaces, or a cf failure.

Readiness check, read-only (exists): `curl -sS -o /dev/null -w '%{http_code}' "$STAGING_ORIGIN/v1/session"` should print `404` or `503`, never `200`. A Router route is refused while the seed Worker is deployed.

## 3. Seed the copies, in batches, on each Mac

**Sunbook** (exists, `verification/central-seed-worker/scripts/`), from the Verifier's frozen v4 export manifest:

```sh
node scripts/build_seed_request.ts "$EXPORTS/manifest.json" "$STAGING_TARGET" "$INCARNATION_I" "$SEEDED_AT_MS" "$EXPIRES_AT_MS" "$PRIVATE/seed-sunbook"
agent-vault --credential "$CREDENTIAL" op run --environment "$ENVIRONMENT" -- \
  node scripts/send_seed.ts copies "$STAGING_ORIGIN" "$PRIVATE"/seed-sunbook/copies-*.json
```

Expected from the builder: `{"projects":67,…,"receivers":["01a16eaa-ae36-7eec-9698-e8c34d10238b"]}`, with every table digest matching the Rust manifest. Expected from each send: `{"kind":"seeded",…}` per project. **Stop** if the builder reports a digest mismatch or any receiver other than C. **Stop** on any send that isn't `seeded`; resend only after reading that Copy's state (the seed refuses a non-fresh object, so a resend can't double-write).

**Sunclaw** (exists; run on Sunclaw by its test agent; its exports never leave Sunclaw): the same two commands against Sunclaw's own frozen exports, writing `$PRIVATE/seed-sunclaw`. Expected `{"projects":3,…,"receivers":["01a16eaa-…"]}`. Sunclaw returns only its `registry.json` (D, M, N, T, E per project; no project data) to Sunbook for the next step.

## 4. Seed the Registry and Fleet, once, merged

Exists:

```sh
agent-vault … -- node scripts/send_seed.ts registry "$STAGING_ORIGIN" "$PRIVATE/seed-sunbook/registry.json" "$PRIVATE/seed-sunclaw-registry.json"
```

Expected: `{"projects":70,"answer":{"kind":"seeded",…}}`. Before writing, the seed Worker checks every listed Copy is Following at the same fence and holder. **Stop** on a refusal naming any project, or a count other than 70.

## 5. Deploy the exact old central over the seeded storage

Exists (dd09aa820), with the old-central artifact from §1:

```sh
node tools/central-migration/scripts/deploy_worker.ts old-central --settings "$S1_SETTINGS" \
  --artifact "$OLD_SOURCE/crates/collaboration-central-cloudflare/build" --scratch "$PRIVATE/deploy-old-central" [--dry-run]
```

- **Expected:** `{"kind":"old-central",…,"namespaces":{"create":[],"keep":[five, by id],"delete":[]},"outcome":"complete"}`, with the same five namespace IDs after.
- **Stop** on `"outcome":"refused"` (exit 1), which names the reason, or on any namespace ID change.

Readiness, read-only (exists): owner `GET /admin/v1/copies` answers 200 with an empty directory, which the seed leaves. For each project, `GET /admin/v1/copies/{id}/health` answers live, stage `following`, receiver bookkeeping 3/3 and no failures. That's the same shape `test/old_central/opens_seeded_storage.test.ts` proves locally. **Stop** on any project that isn't `following`.

## 6. Load the Fleet directory through old central's own import

Exists. The bundle comes from Sunbook's production Router's read-only lists; the Fleet is central's, so they hold all 70 projects:

```sh
node verification/central-seed-worker/scripts/build_fleet_import.ts "$PRIVATE/fleet-import" \
  --installation "$SUNBOOK_SERVICE_ID" --machine "$SUNBOOK_MACHINE_LABEL" --owner "$OWNER_HUMAN_ID"
agent-vault … -- node verification/central-seed-worker/scripts/send_seed.ts fleet "$STAGING_ORIGIN" "$PRIVATE/fleet-import/fleet-import.json"
```

- **Builder output:** `{"counts":{"workspaces":4,"boards":16,"directoryEntries":70},"losses":[2 initiative boards]}`. Those losses are accepted for staging only.
- **Send output:** `{"kind":"imported","counts":{…same…}}`.
- **Read back:** owner `GET /admin/v1/copies`, paged, lists 70 projects, each under its board.
- **Stop** on `refused` (it names each conflict), on any count other than the builder's, or on fewer than 70 listed.

A local dry run of exactly this bundle into old central in workerd already gave imported 4/16/70, with all 70 listed.

## 7. Old 0.1.125 debug Routers, enrollment and the baseline [Verifier]

Use `old-0125-baseline-plan.md` for the source-qualified case routes and `staging-s1-setup-plan.md` for observer arguments. This joint runbook governs sequencing and allocation. Before either Host starts, qualify both Macs' signed 0.1.125 Router/CLI artifacts and their debug-only root/socket flags, the reused installation/peer identities, fresh staging credentials, and provider/model/notification isolation. A release CLI qualified for read-only lists does not qualify a Router for isolated launch. Preserve normal Codex configuration hashes. **Stop** until those launch controls are demonstrated; copied pending deliveries must not reach production providers.

The Router cohort is **4 original Sunbook copies and 3 Sunclaw copies**, per the owner's “we only need 3-4 in each computer” decision (Lead 01a1c1ec). The private selection manifest names the four Sunbook projects and their evidence: two e1/N1 and two e2/N2 files, the largest unsettled handoff backlog and the largest exported project. Central retains all 70 seeded Copies and PD7 still converts all 70; the other 63 are not opened by Routers. Synthetic S1 case projects are declared separately and added to the Router observation cohort when reached.

Each root receives writable copies of only its selected sealed Bs, preserving `sourceRelative`. Keep B read-only/hash-stable outside the root. Reuse the installation identity and peer M; enroll once through old125's actual `fleet enroll` at **`/v1/session`**, with fresh staging credentials. The local synthetic rehearsal caught that an origin-only endpoint can enroll successfully yet leave Fleet writes pending/unreachable; require actual connected Fleet read/write evidence before calling a baseline ready. Keep each exact request/result, reconcile unknown decisions before further writes, and never copy production credentials.

The adopted isolation is a private child `HOME` and explicit private `CODEX_HOME`, siblings under the allocated 0700 parent. Prepare minimal configs and a qualified public managed Codex executable only; copy no user auth/state/sessions. ACP Claude/Cursor are disabled, and the direct Claude peer registry resolves under the empty private HOME. Declare the existing **debug file secret backend** at enrollment (`--allow-plaintext-file-secrets`), keeping the same backend/root through upgrade. This selector exists in old125 and the new140 source; no real Keychain linkage is used. The earlier private-HOME Keychain producer failed and was superseded by Lead 01a1c1c0.

Before any real copy opens, each Mac's synthetic pending-delivery/wake probe must pass: actual private Host environment/config projection, repeated Host/child `lsof` samples with zero real `~/.claude` or `~/.codex` opens, no accepted local delivery and no probe marker at the bound observable sessions. Strict real Codex config hashes stay unchanged. Real Claude heartbeat-byte changes are informational under Lead 01a1c1ca, rather than an unchanged-directory assertion. Sunbook passed with 230 samples and three known-not-submitted wake receipts; its own Codex and the Lead's Claude received no marker. The agent-authored cases below complete the isolation prerequisite on both Macs. The real-copy baseline still awaits direct user approval in the Verifier's session.

The portable procedure uses **agent authorship**: exact commissioned SessionRef in setup actors and explicit agent wake sender/creator, with no human-message mode or inferred identity. Sunbook's separate agent variant passed with 120 samples, three copied Emitted intents and three known-not-submitted agent wakes. Sunclaw's AGENT-v4 case passed according to the Station Chief's report relayed by Lead 01a1c216: 190 samples, zero forbidden opens or missing requested PIDs, three handed-off intents and three known-not-submitted agent wakes. The operator's real session received no marker; Claude and Cursor recipients were synthetic. Product hashes, lock, synthetic B and the real Codex configuration guard stayed unchanged; the controller stopped with exit 0 and ports 18802/18812 were free. Evidence remains on Sunclaw at `/private/tmp/2026-11-s1-isolation-sunclaw-agent-v4/evidence`; the Verifier has not inspected its raw files. Bounded `lsof` sampling is not syscall tracing, and this case covers no real-copy baseline or staging call. The corrected procedure retains every CLI outcome, rejects flags the wrapper supplies, and creates/builds/signs the helper with unchanged before/after product hashes before another Host run. `[[example]]` and dependency edits are unnecessary; keep failed roots and receipts.

`Installation.prepare` creates the fresh private runtime root first. On Sunclaw, the test agent then creates `verification-inputs` under that root (0700), copies the token JSON from the separately parked private handoff to its final input file (0600), and checks the **full** SHA256 against the Lead's private handoff receipt before enrollment. Read source/destination from private settings. The approved age carrier sends only encrypted armor; secret values are never printed or written in a message body. Do not precreate the runtime root for the handoff.

All OS commands against a debug root run through its registered launcher/command wrapper. `Installation.prepare` registers a per-root `verification-commands.jsonl` (0600); bounded CLI reads, enrollment and owned Host/Router children append argv, cwd, environment **key names**, start/end, PID and exit/signal. Values remain out of the ledger. After the successful baseline, `markUpgradeBaseline()` records the identity/enrollment fingerprint and a ledger marker. Commands outside this registered path invalidate complete command coverage; a missing/truncated/pending bounded-command receipt is incomplete.

Require every **selected** held project to open locally and renew at its existing D/M/N, same receiver C and no claim/install/repair/replacement. The seed starts Copy at P=H and preserves S; actual old-Router acceptance remains required. The complete Sunbook copy manifest records **63 e2.sqlite at N=2 and 4 e1.sqlite at N=1**. Each selected placement preserves its original `e<N>.sqlite`; B.sqlite is an intermediate backup name. A project-show reply must report local state, rather than silently falling back to central. **Stop** on a wrong name, refused renewal, different N or replacement.

Reach every synthetic S1 case through the old product before capturing the baseline:

- held projects on both Macs, a completed transfer to N>1, and a separate transfer followed by no new application commit;
- an unheld imported project, a fully retired project, and an archived board/project case, with each public Registry lifecycle recorded independently;
- an accepted native write with measured sender ACK A<H, using only the named owned-debug transport cut; and an observed persisted Fleet checkpoint above zero;
- an Initiative board with goal/doneWhen, a Closed board, an archived workspace, a workspace ADR, and a workspace repository attachment. Keep their full public facts before/after, including the attachment references and ADR content/state. The lossy production Fleet import in §6 supplies none of these synthetic proof cases.

`old-0125-baseline-plan.md` gives the existing import/move/retire routes. The transport cut, numeric checkpoint reader, and ADR/authentication observation adapters must be qualified before their case runs (§10). An unreached case is incomplete. Use synthetic actors and saved UUIDv7 request IDs, without live provider sessions.

Capture fully paged Router Fleet lists and central Registry/Copy facts for the entire declared central cohort. Declare `routerProjects` explicitly in each capture plan/snapshot: the selected seven real copies plus synthetic case IDs when added. Native writer, non-holder content and same-N renewal assertions apply only to this identical before/after Router membership; central grant/Copy/directory preservation stays complete for all 70 plus synthetics. Missing selected evidence remains incomplete, and changing membership between cuts is refused. Unheld/retired synthetic cases keep their separate lifecycle rules. Quiesce application writes; a schema fingerprint does not prove content. Retain source hashes; no HTTP-page/Mac-wide atomicity is claimed.

For S2, independently compare the §6 imported directory against production's public lists, limited to project ID/title → board ID → workspace ID (Lead 01a1c11c). Sunbook's private capture currently contains 4 workspaces, 16 boards and 70 projects from 21 public list commands; its path is in README.md. Capture the corresponding staging lists and reject missing, extra or changed mappings. This limited production→staging comparison does not weaken the full intra-staging S1 graph comparison. **Stop before PD7** if any required baseline/case or directory comparison is incomplete.

## 8. PD7 on staging

**8.1 Migration Worker (step 2).** Exists (dd09aa820): `node tools/central-migration/scripts/deploy_worker.ts migration --settings "$S1_SETTINGS" --artifact tools/central-migration/build --scratch "$PRIVATE/deploy-migration" [--dry-run]`. It keeps the name, bindings and five classes, with only the entrypoint changed. Expected: the same five namespace IDs kept. **Stop** on `refused` or any namespace change.

**8.2 Mac-file facts [Verifier].** Immediately before migrate, collect fresh consistent SQLite online-backup cuts of the **owned live staging debug roots**, on each Mac. Never open a live writer through the SDK, use immutable mode on a live source, or reuse the original production-copy baseline as current staging evidence. Seal each new B (0400), integrity-check it, and derive a fresh writable A from that B. Preserve original B images separately. Source-root qualification must precede this step: `project-backups.ts`'s production-tree CLI is not a staging-root selector; its `backupDatabase` helper supplies the read-only backup operation.

Run the signed retained-writer probe only on A, with B supplied for before/after hash checks. Keep passive original D/M/N/origin/no-loss evidence, a complete public snapshot header/manifest/completion and read-only scratch ledger inspection. The authoritative sender gives C/start/ACK; public `WriterSnapshot.position()` gives head; the approved scratch-A projection of `_fleet_sync_writer.record` at `$.retainedAfter` gives floor. There is no stored head column. No checksum mutation is part of this capture.

The emitter exists: `build-mac-files.ts` consumes a private plan containing `expectedFiles`, `output`, and entries `{database, passive, snapshot, inspection, bSha256}`:

```sh
node verification/observations/central/inplace-migration/build-mac-files.ts "$PRIVATE/mac-files-plan.json"
```

It binds every fact to the same A/B evidence and refuses missing/duplicate identities, receiver/window/lease contradictions or incomplete frames. Merge the two Macs' emitted metadata into exact lossless JSON `{"macFiles":[…]}` and validate it with the migration Worker's `parseMacFiles`. Its membership must equal **all currently held projects**, including synthetic S1 projects; unheld/retired projects do not get fabricated entries. Require floor≤ACK≤head, no needs_snapshot, receiver C, and agreement with the final inventory's holder/lease. Quiesce corpus writes through this cut and the migration call; if facts change, capture a new cut and reconcile before writing.

The 67+3 original-copy body is already validated (README.md); it qualifies the producer, not this fresh staging cut. T3 emission is built; fresh staging capture remains an execution observation.

With only seven real native files opened, preserve original-copy facts for the other 63 central-only projects and refresh the selected seven from staging cuts. The final request still has every held project the migration inventory requires; never fabricate live renewal/window observations for unopened files. Central records and original sealed evidence decide their migration eligibility, while Router renewal claims stay restricted to the selected membership.

**8.3 Inventory, as the dry run** (exists, dd09aa820):

```sh
agent-vault … -- node tools/central-migration/scripts/call_migration.ts inventory "$STAGING_ORIGIN" "$PRIVATE/mac-files.json" --out "$PRIVATE/inventory.json"
```

It POSTs the body and writes the answer once at 0600. It prints `{"step":"inventory","status":200,"success":true,"reason":"no stops, nothing written",…}` and exits 0 only on that.

- **Expected:** `kind: "inventory"`, `stops: []`, `configuredReceiver` = C, 70 projects (each `action: "carry"`, held ones with Copy role, fence, holder, start, acked and schema), `rowsWritten: 0`, and the retiring classes' full dump.
- **Keep** `inventory.json`; it holds the only copy of the retiring classes and is §8.7's input.
- **Stop** on any stop, or `rowsWritten` above 0.

**8.3a The Rust decode gate** (exists, §10 T6), on staging and on production alike:

```sh
cargo run -q -p board-staging-cutover --example migration_decode_gate -- "$PRIVATE/inventory.json" > "$PRIVATE/gate.json"
```

`cargo run` signs the example through the repository's runner. It checks that the inventory's `candidateText` hashes to its `candidateDigest`, rebuilds every candidate object (the Fleet and each Copy, without project tables) in a private in-memory database, and decodes it with main's own decoders: fleet-sync-lease's legacy-row validation, `StoredMachine::decode` per enrolled machine, the canonical Fleet record decoder per workspace, board, ADR and directory entry, the Copy summary and lease projection per directory entry, and fleet-sync-receiver's `copy_status` per Copy. It prints `{"outcome":"pass","candidateDigest":…,"runTimeMillis":…,"refusals":[]}` and exits 0 only on a pass.

- **Keep** `gate.json`; migrate takes its digest and run time.
- **Stop** unless `outcome` is `pass`. Each refusal names the object, the row and main's reason.

**8.4 Migrate** (exists): `call_migration.ts migrate "$STAGING_ORIGIN" "$PRIVATE/mac-files.json" --gate "$PRIVATE/gate.json" --out "$PRIVATE/migrate.json"`. It refuses without the gate's pass. Migrate rebuilds its candidate at the gate's run time before its first write and stops with nothing written unless the digest is the gate's (a candidate changed between gate and migrate is refused). Expected: `kind: "migrated"`, `fleet: "applied"`, every Copy `applied`. **Stop** on `stopped` (it names each stop; nothing was written), or on an interrupted answer: run migrate again, and it resumes from each object's own marker.

**8.5 Verify** (exists): `call_migration.ts verify "$STAGING_ORIGIN" --out "$PRIVATE/verify.json"`. Expected: `ok: true` with every check `ok`. **Stop** on any failed check. Revert is available until 8.6 (`call_migration.ts revert "$STAGING_ORIGIN" --out …`); it's resumable, and re-running it finishes.

**8.6 Main's central (step 5).**

- **Config** (exists): `mainCentralConfig(live, entrypoint)` keeps Fleet and ProjectCopy, gives LeaseRegistryObject, ConnectionObject and MailboxObject the `{"type":"durable-object","state":"deleted"}` tombstone and drops their bindings.
- **Deploy** (exists, dd09aa820): `node tools/central-migration/scripts/deploy_worker.ts main-central --settings "$S1_SETTINGS" --artifact "$MAIN_SOURCE/crates/collaboration-central-cloudflare/build" --scratch "$PRIVATE/deploy-main-central" [--dry-run]`.
- **Expected:** the dry run plans `keep` FleetObject and ProjectCopyObject by id and `delete` the other three. After the run, Fleet and ProjectCopy keep their namespace IDs and the three retired namespaces are gone.
- **Stop** on any change to the Fleet or ProjectCopy namespace IDs. **This step is irreversible**; revert ends here.

**8.7 Copy migration** (exists, `tools/central-migration/scripts/run_copy_migration.ts`):

```sh
agent-vault … -- node tools/central-migration/scripts/run_copy_migration.ts "$STAGING_ORIGIN" "$PRIVATE/inventory.json" --receiver-version 4
```

- **Expected:** one line per Copy. Every live Copy prints `"receiverVersion":4`, every retired Copy prints `"state":"retired"`, and the last line is `{"outcome":"complete"}`.
- **Stop** on exit 1 with `{"outcome":"stopped","reason":…}`.
- 4 assumes the pinned library carries receiver v4 (agent-fleet-sync f052831 or its successor). At 0b27429 the expected version is 3.

**8.8 Router upgrade [Verifier].** Upgrade only the two owned debug Routers to the signed new build, on the same debug roots and enrollments. Never run enroll again, and start no production replacement. Expected: authenticated reconnect, an unchanged machine id, and same-number renewals for each held writer. **Stop** on a re-enrollment prompt, a refused renewal or a reinstall.

Before releasing either Router to reconnect, independently read every §8.7 health result: every live Copy must be readable with receiver stored=supported=4 and no failed migration; every retired Copy must read retired after its explicit repair (retired health reports no version). Match the complete inventory cohort, not only directory pages. Keep full per-request receipts, including pagination and retired repairs. Until this passes, no Mac upgrade/reconnect is released. A different pinned receiver corpus requires an explicit target update, not changing 4 to fit a result.

The S7 and §8 runtime target is **2b3f29d660bcc74915efdd52354bcb7eb63fd244**, version **0.1.140**, with fleet-sync pinned to **bc5935be5f14350be87d32d1c5b272a637c240fa**, frozen at `/private/tmp/2026-11-s1-staging/upgrade-target-2b3f29d66/` (Lead 01a1c267). The Verifier independently checked both executable hashes, strict Developer ID signatures, debug identifiers/team, actual versions and both Worker hashes against its manifest. The earlier d8c628ded and 5480 qualifications are historical and do not cover this changed runtime. Keep the gate and migration Worker heads explicit and qualify their proof separately before inventory/gate/migrate; artifact qualification alone establishes no hosted or S7 runtime behavior.

The adapters take an explicit `{sourceHead, version}` target and each Mac's signed Router/CLI artifacts. Reuse the roots, installation, machine and staging enrollment from §7. `Installation.upgrade({baseline, router, cli, productVersion, sourceHead})` verifies the original enrolled root before stopping owned children, starts the new artifacts on that same root, and checks identity/enrollment again; it contains no enroll operation. It derives zero post-baseline enroll invocations from the per-root ledger. Run each Mac's launcher locally through its named agent; the upgrade does not attach to foreign processes or use SSH.

Capture actual connected `host status` with `host-status.ts` (signed artifact, declared version, exact peer/origin/counters and raw command receipt). That connection read alone reports renewal/enrollment continuity unobserved; require the launcher ledger/fingerprint and actual successful same-D/M/N renewal for every held writer before assembling S7. Old125 `host status` has no Fleet connection field, so its baseline uses actual enrollment/Session/renewal evidence instead of an invented status field.

## 9. S4, S5 and S7 [Verifier]

- **S4 is required on staging.** For every R7 row, use a separately allocated staging copy of old storage with the deliberate contradiction. Run migrate, require the named project/contradiction, total `rowsWritten: 0`, and identical complete table/schema snapshots before/after. Local `stop_conditions.test.ts` is supporting proof; it does not replace this observation. The fixture allocation and exact no-write snapshot driver must be settled before execution; never inject a contradiction into the successful S1 corpus or production to close this case.
- **S5 runs after 8.4 and before 8.6.** Save all table/schema snapshots, repeat migrate with the saved valid body, require the top-level `kind: "alreadyMigrated"` and total `rowsWritten: 0`, and compare every table unchanged. POST inventory must also report zero writes; current migrated inventory answers `alreadyMigrated`, so require its actual response contract rather than the pre-migration inventory shape. Keep raw per-object/statement write counts. A matching summary alone is insufficient.
- **S1 interruption and rollback run before 8.6.** Per Lead 01a1c143-fade, interrupt migrate after some Copies commit and before Fleet commits, then resume: committed Copies must not be redone and none skipped. Interrupt revert partway, resume, then redeploy old central and independently confirm full restoration. Copies migrate first and Fleet last; the Fleet marker means complete. The exact interruption control and table comparison must be qualified before execution. An unknown call is reconciled from markers/readback before any permitted resume.
- **Before reconnect/new writes**, compare carried application schema/content and Copy state with the fixed baseline, using the approved independent public-content/position oracle and the required complete-table checks. The new central must open the migrated objects through its unchanged Rust decoders; retain the edited-migration-file refusal required by S1/R8. Missing routes stay explicit in §10.
- **S7:** take the complete after cut on both Macs and run `migration-observe.ts compare` against the before cut. Require no differences and no missing coverage, including full S1 graph facts, every lifecycle, machine/installation and actual authentication/renewal. A preserved result covers only captured observations. Then make a literal public write on Sunbook and read that exact record on Sunclaw, and the reverse; retain IDs and actual records. Also observe a new Fleet change from the saved nonzero checkpoint without cache clearing or re-enrollment. PD4 preserves the old head as the new floor/head; normal CursorExpired/rebuild is allowed, CursorInvalid is not. The native-tail case needs its separately declared accepted-record arrival oracle; do not weaken the quiescent content comparer to hide its intentional new record.

## 10. Gaps before the first staging command

|     | Gap                                                                                                                                                                                                                  | Owner                                                                   |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| T1  | Built in dd09aa820: one TypeScript guarded deploy for the four Workers, with dry run and planned diff.                                                                                                               | Implementer                                                             |
| T2  | Built in dd09aa820: `call_migration.ts` for inventory, migrate, verify and revert.                                                                                                                                   | Implementer                                                             |
| T3  | Built and exercised: `build-mac-files.ts`, signed retained-writer snapshot and scratch inspection. Original-copy body accepted at 70 unique entries; fresh staging cuts remain required at §8.2.                     | Verifier                                                                |
| T5  | The allocation: the Lead checks `/private/tmp/2026-11-s1-staging/s1-settings.json`; then the Access applications and service tokens are created (C3 pattern) and the debug roots and ports filled.                   | Lead / Implementer / Verifier                                           |
| T6  | The Rust decode gate (§8.3a): a verification-only example in board-staging-cutover.                                                                                                                                  | Implementer                                                             |
| T7  | Built/tested at current0b: --fleet-checkpoint uses public FleetCache on fresh A with sealed B hashes. Scope is granted; final f052 qualification and actual staging checkpoint>0 remain.                             | Verifier                                                                |
| T8  | Authored: declared-target graph/Host-status adapters, per-root ledger and existing-root Installation upgrade. The 2b3f29d66 artifacts are qualified; actual reconnect and same-N renewal evidence remain.            | Verifier; Lead/Implementer declare target                               |
| T9  | Exact owned-debug transport cut and accepted-tail arrival oracle; provider/model/pending-delivery isolation must be demonstrated before real copied roots start. No production networking or system firewall change. | Verifier investigates existing controls; Lead settles any new mechanism |
| T10 | Full S1 graph observation: public ADR/repository readback and preservation; complete-table no-write/content checks; qualified migrate/revert interruption controls and edited-migration refusal.                     | Verifier observation; Implementer supplies execution control            |
| T11 | S4 deliberately contradictory old-storage staging fixture, kept separate from the successful S1 corpus, with every R7 case and complete before/after table snapshots.                                                | Lead allocates; Implementer prepares; Verifier observes                 |

None of these is staging work. Each lands and is reviewed before the step that uses it.
