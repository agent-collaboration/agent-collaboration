# Old0.1.125 baseline and complete S1 case matrix

Use [staging-runbook.md](staging-runbook.md) for governing sequencing, allocation, current adapter status and execution gates. This document supplies source-qualified baseline/case routes. Its earlier driver/build prerequisites below are historical where the joint runbook marks them built. Staging has not run.

This is a procedure for the Lead's staging release, not evidence that staging ran. D4 stays paused. Governing sources are the adopted migration Verification Design S1/S2/S7 and Lead01a1c0cc/01a1c0d3. The old central is2636806a588916347044829971cfacc003a1c937; the old Router/CLI tag iscbf56bad640ae981900c646ff2ae0b64aaaebd3b (0.1.125). Source references below are at those pins, not a current parser assumed compatible.

## Cohorts and order

Keep separate case IDs for the real-copy cohort and the deliberately constructed S1 cohort. The current real-copy evidence covers67 Sunbook files and3 Sunclaw files; all70 are at head, with no unacknowledged tail. That closes no synthetic-tail case. Source rows stay on their Mac; Sunclaw uploads its three Copy images directly to the allocated staging origin with its own staging Access credential. Only metadata and receipts cross the coordination channel.

The order is allocation → seed Worker → both Macs' Copy batches → one complete Registry/Fleet initialization → exact old central → fresh real enrollment under reused identities → old125 opening/renewal baseline → synthetic S1 setup → independent before cut → PD7 → new central/135 → independent after cut/S7 → new writes/readback.

The seed Worker's current source separates CopiesRequest and RegistryRequest (`verification/central-seed-worker/src/seed_contract.ts`). Copies requests carry pages; the final Registry request carries all projects' compact facts and incarnation. Its driver invocation is not invented here: the Implementer must hand back its settled batching command and request schema. Wait for both Macs' batch receipts before finalizing Registry. An uncertain partial seed is reconciled from receipts, never blindly replayed into non-fresh objects. The existing five class namespaces must be the allocated ones across every deployment.

**Do not initialize the old Registry/Fleet with a readiness call before the seed step.** The seed Worker requires fresh objects. Freshness/reset/allocation is the deployment operator's explicit responsibility; this runbook does not authorize namespace replacement or deletion. After seeding, redeploy exact old2636 over those same objects.

## Required allocation and isolation packet

Before any launch or write, name and qualify:

- Staging origin/name, existing namespace IDs/class bindings, exact old/seed/migration/new bundle heads and hashes, and the approved deployment wrapper/config. No production target.
- Each Mac's signed125 Router, agent-collaboration and board-upgrade artifacts: version, hash, strict signature and identifier/team. Obtain135 artifacts separately for the later upgrade. A tag or Homebrew path alone is not qualification.
- One fresh private debug root/port allocation per Mac, its real source installation/peer identity mapping, new staging credentials, owned child-process receipts, isolated operator and app-server sockets, and normal Codex config hashes.
- A source-qualified way to disable provider/model input and copied pending notification delivery before opening real copied project data. Default provider configuration is not assumed safe isolation; no live provider session is an S1 actor.
- Per-project B path/hash and original `sourceRelative` from the copy manifest: Sunbook has63 e2.sqlite at N=2 and4 e1.sqlite at N=1. Preserve each original basename on placement; never infer it from the intermediate B.sqlite name. Ordinary125 opening and same-N renewal remain required.
- A private independent recorder plan, full-directory/content cohort manifest and actual S7 observation route. Authentication cannot be filled in by hand.

The previous launch/deployment recipes remain in `staging-s1-setup-plan.md`. Confirm every flag with the declared125 artifact before use. `fleet status` and `host status` have no --json option; their actual output may still be JSON and must be parsed at this version.

## Root preparation and old125 enrollment

Write only the existing installation/peer JSON values into each fresh debug collaboration directory, under the identity-file contract in `staging-seed-worker-spec.md`. Never copy production credentials. Enroll once using the actual product route and newly allocated staging Access material. Source:125 `fleet_command/arguments.rs:51–78`, `enrollment_prepare.rs:84–110`.

```text
ROUTER125 fleet enroll --router-root DEBUG_ROOT --endpoint STAGING_ORIGIN --label TEST_LABEL --access-client-id ALLOCATED_ACCESS_ID --access-client-secret-file PRIVATE_ACCESS_FILE --expires-at APPROVED_EXPIRY --owner-token-env MIGRATION_OWNER_ASSERTION
```

Use the existing secret-safe wrapper; the final environment name is a name, not a token in argv. The debug-only --allow-plaintext-file-secrets flag is used only if the Lead's isolated-root secret-storage allocation explicitly selects that mode. Do not re-enroll after migration. Reconcile any uncertain enrollment decision/publication before another mutation.

Record the enrolled peer, installation, authoritative receiver and expiry. Require the receiver to equal the copied senders' registered receiver; do not add a second sender registration to conceal a mismatch. Both Macs' currently observed sender receiver is the same, but live config/enrollment must still be read and checked.

## S1 cases, reached through existing product boundaries

| Case                        | Setup boundary                                                                                                                       | Deciding before facts                                                                                                              | Required after facts                                                                                                                |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| Ordinary held on both Macs  | 125 public workspace/board/project/role/workstream/task/thread writes                                                                | Registry heldM,N; CopyM,N,S,H/schema; current local writerM,N; full non-holder records                                             | Same grant/data; same-N renewal and later new writes/readback                                                                       |
| Transfer above1             | 125 central project move with saved transfer UUID                                                                                    | Transfer complete, destination writer at N>1, source no longer current                                                             | Same destination/N; no fresh allocation/claim/install                                                                               |
| Transfer with no new commit | A separate normal move, followed only by reads                                                                                       | New holderN, no application write after move; SDK H and S captured, including H.epoch<N when that is the actual result             | Same N and exact H/S/manifest; no epoch normalization                                                                               |
| Unheld                      | Existing125 board-upgrade CLI publishes a sealed synthetic legacy fixture through owner import; leave one imported project unclaimed | Public Registry unheld, authoritative Copy holdernull/readable; explicit null-holder public content view; no current native writer | Remains unheld, same Copy/content, no invented writer/renewal                                                                       |
| Fully retired               | 125 central project retire with the observed fence/ACK and saved request UUID                                                        | Actual fully Retired reply and public retired Registry/Copy; Pending is not completion                                             | Remains retired; no new held grant or replacement Copy                                                                              |
| Archived                    | 125 board archive on a dedicated synthetic/imported board                                                                            | Public boardArchive archived plus the project's independently observed Registry lifecycle                                          | Same archive and lifecycle. Archive is not guessed to mean unheld; use the unheld project in that board if the case needs both axes |
| Native unacknowledged tail  | Bounded normal local write while only the owned debug Router is cut off from staging                                                 | Accepted local record plus measured sender ACK A<H and captured manifest; never infer a tail from a disconnected badge             | Existing data preserved and the exact tail reaches Copy/other Mac after reconnect, with same lease                                  |
| Saved Fleet feed above0     | Public Fleet writes/readback, then an independent saved-checkpoint observation                                                       | Actual persisted checkpoint>0, recorded with the old central head                                                                  | Normal new-client feed handling has no CursorInvalid; directory view preserved/rebuilt as PD4 specifies                             |

Use the actual old CLI forms for ordinary setup and move in `staging-s1-setup-plan.md`. The parser for owner move/retire is125 `agent-collaboration/src/central_commands.rs:49–72`. Retirement adds:

```text
AC125 central project retire PROJECT_UUID --central STAGING_ORIGIN --request-id SAVED_UUIDv7 --expect-fence OBSERVED_N --expect-acked EXACT_POSITION_JSON --json
```

The archived-board form is `AC125 board archive --board-id BOARD_UUID` plus the same request/actor/service-directory options as other board mutations (`board_arguments.rs:109–122,418–425`). `project claim` exists (`board_arguments.rs:149–164,582–588`) but is used only for a deliberately selected synthetic imported project; never on any real-copy baseline project or on the case that must remain unheld.

### Existing unheld import path

The old125 tag already contains `codex-router-board-upgrade`, package board-legacy-upgrade. Do not substitute the newer board-staging-cutover CLI, which is absent at the125 tag. The existing import command accepts --source, --report, --router-directory, --owner-human-id, --machine-label, --dry-run, --central-url and --owner-credential-file (`board-legacy-upgrade/src/import_command.rs:16–40`). It requires the owned debug Host to be closed; schedule this before that Host starts.

Use a private read-only copy of the committed synthetic `crates/board-legacy-upgrade/tests/fixtures/legacy-complete.sqlite` from the old tag, with its source/copy SHA256 equal. No production legacy store. Run the tool's dry run first; then, under the staging release, publish to the exact old central with the owner credential handoff. Actual old product E2E `agent-collaboration/tests/board_e2e/central_import.rs:12–50` uses this CLI and deliberately leaves an unheld project. The old central exposes POST /admin/v1/import/fleet and /admin/v1/import/projects/{id} (`central_router.rs:52–58`); this is not the conformance-only imported_board_snapshot helper.

Keep the native report/import IDs and actual public observed lifecycle for each resulting project. Do not guess fixture IDs or invent transfer receipts. Choose one project to remain unheld, another for deliberate claim if needed. Imported fresh staging lineage is not original production Registry history.

### Tail and saved-feed route gates

The old125 Fleet CLI exposes enrollment/status, not a live disconnect command. An owned debug-only network cut is permitted by Lead01a1c0d3, but its exact existing control/transport route must be named before execution; no system firewall, production disconnect or invented central route. Require the normal local write to be accepted under its existing valid lease and an actual A<H observation. If disconnected writes refuse or the tail drains before the cut, record that attempt as not reaching the case; do not manufacture a sender ACK or mark it passing.

A tail case must not make the equal-content oracle silently ignore differences. Preserve the original baseline records exactly, and separately name the one accepted new record/position expected to arrive. The ordinary quiescent before/after comparer remains strict. If the intended interleaving cannot use that unchanged oracle, bring the deciding observation contract to Lead before execution.

For the numeric Fleet prerequisite, the public library already has `FleetCache::checkpoint()` and `FleetCache::open()` at125 `message-board-storage/src/fleet_cache.rs:69–76`. A public CLI numerical checkpoint read has not been located. A verification-only public-library read of a fresh routing-database A copy can observe it without private SQL, but that adapter's new routing-file scope still needs a binding. Normal list output alone is not proof of a persisted numeric checkpoint. PD4 keeps new Fleet head/floor at the old head; it does not set head to0. Do not invent a runtime cache reset or require equal before/after checkpoint numbers.

## Mandatory old125 baseline for real copies, before PD7

1. After both Macs' Copy phase and the single complete Registry phase, redeploy exact old2636. Confirm same allocated class/namespace inventory and old bundle; keep unknown deployment effects reconciled.
2. Install only copied B files into their allocated debug project locations, preserving source relative names and file identities. B remains sealed until its intended old Router opens it; retain original hashes/manifests and observer artifacts. No claim, repair, import or reinstall of these real project IDs.
3. Launch only the declared old125 debug processes. Require actual authenticated staging Session, existing file accepted, same D/M/N renewal, same sender receiver, and unchanged physical identity/replacement evidence for every held project. A current local holder label or original registration receipt alone is not renewal proof.
4. If any original file refuses through the old Router, first check its placement against the original manifest (63 e2/N2 and4 e1/N1), then stop and report the actual failure before a mutation or workaround. The supplied extractor opening context does not close this product baseline.
5. Require every seeded Copy's D/role/N/M/S/H/schema and complete content to match its local image, then actual non-holder public views. Native observers run on fresh A cuts, never by opening a live writer file alongside the Router. Renewals and sender pruning can legitimately change native bookkeeping; recapture source-bound facts after the baseline settles.
6. Generate the migration body's exact macFiles for every held project at that final cut: database, holder, lease, senderReceiver, retained.floor/head. Floor is `_fleet_sync_writer.record` at JSON path `$.retainedAfter`; head is public WriterSnapshot.position() computed from capture state, not a nonexistent stored head column. Keep the precise source/hash provenance. The existing67-file copy-baseline fragment must be refreshed if the staging writer changed.
7. Capture all Router lists, every public lifecycle/lease and Copy health, and the declared content using the independent recorder. `lease-coverage.ts` treats held/unheld/retired separately and compares stable Registry grants. Missing Registry publication/renewal/content stays incomplete; a migration report cannot fill it.
8. Only after the complete S1 case matrix and old125 baseline are reached does the Lead release PD7. Later135 authentication/renewal and bidirectional writes remain S7/R4, as in the parent runbook.

## Outstanding execution gates

Final seed driver invocation/batching and input conversion; allocated fresh objects/private deployment config; qualified old125/135 binaries on both Macs; routing-only/provider-isolated launch; source-qualified live authentication/renewal readback; exact debug-only tail cut; numeric saved-feed observer. The newer native ledger-adoption proof is also required before staging under Lead01a1c0e1/01a1c0fe. No one has executed this runbook or closed those gates by writing it.
