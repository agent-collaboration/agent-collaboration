# S1 staging setup and S7 continuation

The governing procedure is now [staging-runbook.md](staging-runbook.md). This document supplies observer arguments and source pointers. Its earlier deployment/allocation sequence is historical: the Lead allocated a new `collaboration-central-s1-migration` Worker whose first upload creates five fresh namespaces; preserve those IDs on every subsequent deploy. Use the joint runbook's TypeScript wrappers, current proof status, full S1 case matrix and execution gates.

**Phase0 plan only — no command below has deployed or mutated staging.** Deployment and initial enrollment/project creation wait for the fleet-access Lead's explicit staging allocation/availability and reviewed design. S1 then runs on both Macs; Sunclaw actions go only through its named test agent. D4 remains paused. Production is never the target of this runbook.

## 0. Declare the setup before making it

The operator supplies a private allocation: full old/new central heads, signed old/new Router and agent-collaboration artifacts, staging origin/name/account/profile/config directory, existing staging namespaces and class bindings, fresh debug roots/ports for Sunbook/Sunclaw, owning human test identity, private evidence directories, and an agreed readiness deadline. Record the exact source/config/bundle/signature hashes without credentials. No account, token or credential path belongs in a public packet.

Pins resolved locally:

- production central **2636806a588916347044829971cfacc003a1c937**. Its workspace version says0.1.119; do **not** replace this source with a0.1.125 central merely because the old Mac Routers are0.1.125.
- Router0.1.125 tag resolves **cbf56bad640ae981900c646ff2ae0b64aaaebd3b**; Router0.1.135 tag resolves **e3d2a013728b05f3f42804238ce84cf7b1533e76**. These are local Git ref resolutions, not qualified binaries or acceptance of the yet-reviewed migration build.
- migration worktree initially **b2a0d22809580048102e3725736a5a554855672c**. The new central/Router runtime for the live run must be re-declared after reviews; a mutable worktree is not its pin.

Use TypeScript preparation/orchestration and existing binaries. Source extraction is `git archive --format=tar --output <PRIVATE_ARCHIVE> <FULL_HEAD>` followed by extracting into a fresh owned checkout; record archive SHA256 and verify tracked source against the chosen head. This is a preparation recipe, not permission to create arbitrary worktrees or discard changes. No stash, reset or history rewrite.

## Staging allocation

- **Authoritative receiver C: `01a16eaa-ae36-7eec-9698-e8c34d10238b`.** This is an identifier, not a credential. Every one of the 67 Sunbook v4 exports names this one receiver as the authoritative receiver its sender is registered with; the seed request builder (`verification/central-seed-worker/scripts/build_seed_request.ts`) reported this single value. Staging's old central must be configured with exactly this `authoritativeReceiver` in `CENTRAL_REGISTRY_CONFIG` (seed spec §1, C). The migration Worker reads the same binding at step 2 and compares it with each Mac's sender receiver before migrating. Sunclaw's three projects must report the same C; a different value is a stop, not a reconfiguration. Added by the Implementer at the Lead's direction, 2026-11-09.

## 1. Build the exact old central, without test features

From the extracted old source's monorepo root, the established build commands are:

```sh
worker-build crates/collaboration-central-cloudflare --profile worker-release --no-opt -- --locked --no-default-features
node verification/tooling/check-central-worker-bundle.ts production
```

Use pinned worker-build0.8.7 and the source's dependency lockfile; record JS/WASM hashes. `crates/collaboration-central-cloudflare/README.md:11–17` owns these arguments; no conformance-faults or clock shim in the production-equivalent bundle. Native executables actually launched must first pass strict signature/hash/team checks; a build is not a staging observation.

Old `cloudflare.config.ts` has **five** existing SQLite classes: ConnectionObject, ProjectCopyObject, LeaseRegistryObject, FleetObject, MailboxObject. The operator pins actual namespace IDs and names, not merely export spellings. Never deploy main's two-class template as the old version. Never create a new PROJECT_COPIES namespace to make deployment convenient.

Fill the approved private staging cf configuration with that old guarded bundle and staging-only Access/registry/delivery values. Keep workersDev/preview URLs disabled as in the old template. Preserve `wrangler.config.ts`'s noBundle/CompiledWasm rules. The operator's TypeScript wrapper runs the following exact CLI argv **from the approved private staging config directory**:

```sh
cf deploy --mode staging --profile "$STAGING_PROFILE" --tag migration-s1-old-2636806a5 --message 'S1 exact old central 2636806a5'
```

This is the verified `cf deploy --help` syntax, but the **private config location/mode and upload mapping are still supplied by the fleet-access Lead**; no usable account/config has been invented. The wrapper must refuse if the evaluated worker name/account/routes/class namespace IDs differ from the staging allocation. Use the existing repo/ops secret-safe `op run` wrapper when credentials are needed, never put resolved values into arguments/evidence. Do not add `--prebuilt` solely because worker-build ran: that flag expects cf Build Output API files, not an arbitrary Rust `build/` folder.

After a successful deploy, retain Cloudflare version/deployment inventory showing the old bundle/class bindings at the staging origin. Read-only HTTP readiness and a real authenticated Session, not elapsed time, establish readiness. Failed/unknown deploy effects require inventory reconciliation; never blindly repeat the deploy. The exact private operator wrapper/config remains an execution dependency.

## 2. Create isolated old debug Routers on both Macs

Prefer already available **signed**0.1.125 debug artifacts qualified by path/hash/version/identifier/team. Never install them over Homebrew or replace the production process. If they must be built, use the tag source and normal target in a separately coordinated Cargo window; signing is mandatory before launch:

```sh
cargo build --locked -p codex-router-cli -p agent-collaboration
scripts/cargo_debug_signing_runner.sh --sign-only <PRIVATE_CODEX_ROUTER> <PRIVATE_AGENT_COLLABORATION>
<PRIVATE_CODEX_ROUTER> --version
<PRIVATE_AGENT_COLLABORATION> --version
```

The TypeScript launcher uses `execFile`/`OwnedProcess` argument arrays, with cwd at the extracted source root. Debug-root paths and ports below come from the approved private allocation, never production defaults. Existing `verification/machines/installation.ts` supplies the source-backed argument shapes:

```sh
<ROUTER_0125> serve --port <ALLOCATED_PORT> --state-db <DEBUG_ROOT>/state.sqlite --secret-root <DEBUG_ROOT>/secrets --require-debug-isolation --disable-background-quota-refresh
<ROUTER_0125> host --router-root <DEBUG_ROOT> --port <ALLOCATED_PORT> --mcp-bind 127.0.0.1:0 --require-debug-isolation
```

Set the debug-only app-server socket to `<DEBUG_ROOT>/n/app-server.sock` in a private owned directory; keep normal Codex home and config untouched. Record Codex config fingerprints before/after, as existing verification guidance requires. Avoid a second app-server startup racing the installed Host; resolve its isolation/launch prerequisites with the operator before starting. These commands are launch instructions, not authorization to run a model or create provider sessions.

Before launching the enrolled Host, use the version's existing `fleet enroll` flow at the explicit debug root and staging endpoint, once per Mac, through the approved credential wrapper. Exact source-backed flags:

```sh
<ROUTER_0125> fleet enroll --router-root <DEBUG_ROOT> --endpoint <STAGING_ORIGIN> --label <SYNTHETIC_MACHINE_LABEL> --access-client-id <ALLOCATED_ACCESS_ID> --access-client-secret-file <PRIVATE_FILE> --expires-at <APPROVED_ISO_EXPIRY>
```

Verify these flags with the **declared0.1.125** binary's help before execution; the existing `verification/clients/fleet-enrollment.ts` is the implementation pattern. No production credential copying. Record only enrollment decision/machine and installation IDs, never credentials. Enrollment is setup and must not run again during the migration/upgrade. Unknown enrollment effect is reconciled before another request.

Causal readiness checks:

```sh
<ROUTER_0125> fleet status --router-root <DEBUG_ROOT>
<ROUTER_0125> host status --router-root <DEBUG_ROOT> --port <ALLOCATED_PORT>
<AC_0125> endpoints list --service-directory <DEBUG_ROOT>/agent-communication --json
```

`host status` and `fleet status` have no `--json` flag in the inspected CLI; do not append one. Their actual output and successful authenticated central Session must be captured by a version-bound S7 adapter. No model input is needed to create synthetic board data.

## 3. Make a nontrivial S1 corpus, using normal public writes

Both enrolled debug Routers create projects they initially hold. Use at least three project UUIDv7s: two held by Sunbook, one by Sunclaw, then normally move one Sunbook project to Sunclaw. Seed a workspace, an open board and a deliberately archived board represented in the directory; each project has literal task/workstream/post content. Retain the exact write requests and accepted results. Every project must have a non-holder on the other Mac who can read its complete public view through central before migration.

The installed help and source (`board_arguments.rs`, `board_entity_arguments.rs`) confirm these argument forms; the declared0.1.125 binaries must confirm them before the staging writes:

```sh
<AC_0125> workspace create --workspace-id <UUIDv7> --name migration-s1 --description 'Synthetic migration verification' --request-id <UUIDv7> --actor <TEST_ACTOR_JSON> --service-directory <DEBUG_SERVICE> --json
<AC_0125> board create --kind '{"kind":"area"}' --workspace-id <WORKSPACE_UUID> --board-id <UUIDv7> --name migration-s1 --request-id <UUIDv7> --actor <TEST_ACTOR_JSON> --service-directory <DEBUG_SERVICE> --json
<AC_0125> project create --board-id <BOARD_UUID> --project-id <UUIDv7> --title migration-project-a --request-id <UUIDv7> --actor <TEST_ACTOR_JSON> --service-directory <DEBUG_SERVICE> --json
<AC_0125> role grant --request-file <PRIVATE_TYPED_ROLE_REQUEST> --service-directory <DEBUG_SERVICE> --json
<AC_0125> workstream create --project-id <PROJECT_UUID> --workstream-id <UUIDv7> --name preserved-stream --goal 'same content after migration' --request-id <UUIDv7> --actor <TEST_ACTOR_JSON> --service-directory <DEBUG_SERVICE> --json
<AC_0125> task create --project-id <PROJECT_UUID> --workstream-id <STREAM_UUID> --task-id <UUIDv7> --title preserved-task --body 'literal task content' --request-id <UUIDv7> --actor <TEST_ACTOR_JSON> --service-directory <DEBUG_SERVICE> --json
<AC_0125> thread create --about 'project:migration-s1' --project-id <PROJECT_UUID> --post-id <UUIDv7> --text-file <PRIVATE_LITERAL_POST> --request-id <UUIDv7> --actor <TEST_ACTOR_JSON> --no-watch --service-directory <DEBUG_SERVICE> --json
```

Role request: use the existing typed `role_grant` shape from `verification/observations/central/d1-projects.ts` (project subject, lead role, exact actor, expectedRevision0 and null model/lineage/ownerGrant) with `writeHeader` request/protocol/actor fields. The actor must be the authorized synthetic owner or a separately commissioned test identity; never impersonate a human or notify a live provider. If the actor is a session, use the proper thread role and keep notification-causing helpers unloaded; do not invoke the existing D1 journey wholesale, which also performs unrelated deliveries.

Move exactly one held project while both machines are connected:

```sh
<AC_0125> central project move <PROJECT_UUID> --central <STAGING_ORIGIN> --transfer-id <SAVED_UUIDv7> --to-installation <SUNCLAW_INSTALLATION_UUID> --reason 'S1 preserve lease above one' --json
```

Observe the explicit result, directory holder and both writer records until the same transfer completes; no sleep creates readiness. Keep request identity on a known idempotent continuation; uncertain mutation is never blindly replayed. Require that the final holder is Sunclaw and its lease number is **greater than1**. Do not use takeover/retire as a shortcut. Populate content only through normal public writes and qualify sync by actual non-holder reads, not a timeout treated as success.

## 4. Capture the independent before cut

Make no further corpus writes. Wait causally until every expected content record and copy acknowledgement is observed. Pin the complete enumerations and reject unavailable/truncated results.

On each Mac, record all Router lists and the other Mac's projects' public content with the signed CLI:

```sh
node verification/observations/central/inplace-migration/record-router-lists.ts <PRIVATE_BEFORE_ROUTER_PLAN>
```

Read every held project's local writer using the separate retained-open adapter, and capture its source/hash. This adapter remains a prerequisite; no CLI command was fabricated for it. Assemble the typed independent Router observations with actual machine/installation identities, version, list/content evidence and writer tuples, leaving before authentication `notObserved` if S7 has not happened. Missing evidence stays incomplete.

Capture central:

```sh
node verification/observations/central/inplace-migration/migration-observe.ts record <PRIVATE_BEFORE_CENTRAL_PLAN>
```

Expected: nonempty fully paged directory; one health response per project; database/role/fence/holder/start/acked/schema and schema-only projectFingerprint; every workspace/board/directory fact and machine identity from the Router pair. A before snapshot does not assert a complete registry/orphan inventory.

## 5. Migration, after cut and S7 — later phase

Execute reviewed PD7 under its separate authorization: migration Worker deploy/inventory/approved migrate/verify, then the new central at the exact existing namespaces, retiring classes only at the reviewed boundary. S4 contradictions and S5 replay/no-write proof are separate tests, not supplied by this recorder. S3 production inventory is later and has no Phase0 authorization.

Upgrade only the two **owned debug** Routers to the signed declared0.1.135 build, reusing their debug roots and enrollments. Start no production replacement. Require actual authenticated reconnect, unchanged machine IDs/credentials (no enroll invocation), successful same-number renewals for each held writer and unchanged directory/content. Take the after cut **before** S1's new writes, then:

```sh
node verification/observations/central/inplace-migration/record-router-lists.ts <PRIVATE_AFTER_ROUTER_PLAN>
node verification/observations/central/inplace-migration/migration-observe.ts record <PRIVATE_AFTER_CENTRAL_PLAN>
node verification/observations/central/inplace-migration/migration-observe.ts compare <PRIVATE_BEFORE_SNAPSHOT> <PRIVATE_AFTER_SNAPSHOT>
```

Preservation requires no forbidden differences and no missing coverage. Equal copy positions/schema plus non-holder view digest close the approved R3 public-content proof; schema fingerprint alone does not. Then do a new literal public write on Sunbook and read it on Sunclaw, and the reverse, retaining request/event/record IDs. No receipt/deploy/HTTP-only result is upgraded into authentication, cross-Mac sync or production acceptance.

## Remaining execution prerequisites

Fleet-access Lead's staging allocation/private cf config and deployment wrapper; review-adopted target; exact signed0.1.125/0.1.135 artifacts; Sunclaw response/availability; retained-writer decoder and consistent S2 snapshot cut; source-qualified public non-holder routing; S7 actual-output adapter. They are named gaps, not a reason to replace the independent oracle with migration reports. No wall-clock outcome waits, model input, secrets in reports, gitstash or production changes.

## Sunclaw planning handback (not execution proof)

Operator reply01a1c082-769f-7aa3-b92f-f40db2cdbfcf returned a signed installed0.1.125 candidate with strict verification0; it did not execute --version. No qualified0.1.135 artifact was found in its scoped search. Exact signature/hash and limits are retained in sunclaw-phase0.json. Historical link-health debug builds and revoked/old staging roots are not substitutes. Provider launch disablement, normal-Codex profile/socket isolation and matching135CLI must be settled before launching. No Sunclaw copy/build/enrollment/status/lifecycle/staging action was requested or performed in this phase. Lead's later SQLitebackup decision supersedes that handback's then-unsettled copy mechanism.
