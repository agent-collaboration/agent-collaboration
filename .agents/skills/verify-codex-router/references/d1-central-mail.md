# D1: one Fleet store and pushed mail

Use this journey for central-storage-reset D1. The oracle is `docs/specs/2026-11-02-central-storage-reset/verification-design/d1-central-layout-and-mail.md`, corrected at `c133916d8`, and its linked central-layout and mail specifications. Plan r4 schedules the observation controls. This recipe grants no staging, deployment, enrollment or model input.

**Coverage remains partial.** The preparation entry prints the deciding checks and eleven open outcomes; it resolves no credentials, starts no process, makes no request and writes no evidence:

```sh
node verification/run.ts --help
node verification/run.ts --d1-plan
```

`verification/observations/central/d1-coverage.ts` owns the executable check inventory. O3 includes every failure-table case; O11 requires both never-enrolled socket refusal and revoked-machine Submit refusal. A row receives observation credit only when all its checks have retained evidence. O5/O6 stay open on Tokio even if a local stand-in supplies results. The current two-machine legacy declaration and OB4 row parser cannot execute D1.

## Native startup without input

The verification-only Rust executable has a separate, bounded startup qualification:

```sh
cargo build -p codex-router-tailnet-verification-host --locked
scripts/cargo_debug_signing_runner.sh target/debug/codex-router-tailnet-verification-host d1-native-ready "$D1_NATIVE_PLAN"
```

Build only after the shared build owner releases the slot. The owner-private JSON follows `D1NativeReadinessPlan` in `crates/tailnet-verification-host/src/d1_host_plan.rs`: canonical repository, fresh direct child of `tmp/d1-verify/allocation/`, exact declared native path/hash/version, explicit auth policy, timezone-aware deadline. The policy variants are `awaitingOwner`, `sharedDebugProxy` with a loopback origin, or `dedicatedDebugAccounts`. Pending policy is allowed only because this entry sends no model input.

The package's launcher test starts `CARGO_BIN_EXE` directly. Sign that rebuilt helper before the tests; the Cargo runner's test-executable signature does not sign a helper launched by the test. Cargo can restore the linked artifact over the public path even without recompiling. Set `D1_LINKED_HELPER` to the current build's actual executable `target/debug/deps/codex_router_tailnet_verification_host-<hash>` (not its `.d` file or a test executable), sign both paths, and verify the public signature again after Cargo:

```sh
scripts/cargo_debug_signing_runner.sh --sign-only "$D1_LINKED_HELPER" target/debug/codex-router-tailnet-verification-host
cargo test --locked -p codex-router-tailnet-verification-host
cargo clippy --locked -p codex-router-tailnet-verification-host --all-targets -- -D warnings
cargo fmt -p codex-router-tailnet-verification-host -- --check
```

The driver copies only the declared executable bytes into its fresh Codex home, writes an ephemeral-auth configuration with `requires_openai_auth=false`, and clears inherited credential environment. It sends `initialize`, `initialized` and `account/read` with `refreshToken=false` over stdio. Require `account=null`, `requiresOpenaiAuth=false`, zero observed connections to its held loopback upstream guard, no auth file, exit zero and an errors-free empty cleanup inventory. Keep `qualification-plan.json`, `native-ready.json`, `host-started.json`, `host-settlement.json` and diagnostics. This qualifies stdio native startup only: no Unix endpoint, Router, model access or D1 observation receives credit from it. The existing Tailnet mode still uses its own plan and normal-home guards.

## Three-Host Tokio driver

The maintained executable entry is:

```sh
node verification/run.ts --d1-harness "$D1_HARNESS_DECLARATION"
```

Use an owner-private JSON file matching `d1HarnessSchema` in `verification/observations/central/d1-harness-plan.ts`. Declare `suite: "d1-harness"`, canonical `repository`, signed `driver` and `cli` artifacts (path, SHA-256, signing identifier and team), declared `native` bytes/version, a fresh `evidenceDirectory` under `tmp/d1-verify/`, a separately authorized loopback `debugProxyOrigin`, and a live UTC `deadline`. The driver is `codex-router-tailnet-verification-host`; the CLI is the matching debug `agent-collaboration`. Capture signed immutable artifacts after the compiling window and qualify them again at launch. No standing Router is started by this entry.

`queueScenario` selects `none` (default), `online-held`, `offline-reopen`, or `online-timing`. `twoProjects` defaults to false; true creates P1 on A and P2 on B through public board calls, grants each real Session its first lead role and reads back a workstream marker. Unload each actor and observe the empty native loaded-thread list before notifying writes. Pending board operations retain their original request ID; a timeout fails the drive and must not generate a replacement operation.

The Rust driver launches an enrolled Tokio central and three independent native-backed debug Hosts. It uses fresh Router/Codex roots under `tmp/d1-verify/allocation/` and the commissioned short service directories `tmp/a`, `tmp/b`, `tmp/c`. Before another run, require the previous owners settled and their exact directories unused; preserve previous state under the task root rather than overwriting it. It retains real SessionGate Welcome frames, public installation identity, Unix native/schema readiness, actual Fleet socket-owner inventory and real `gpt-6-luna` thread metadata. It parses generated routing configuration and requires the single declared debug provider, ephemeral credentials and first-party auth disabled. These are startup and routing observations, not model-service readiness.

`offline-reopen` stops B, sends one public queue message from A, reads sender Sent while B is offline, reopens B against the same stores without loading its recipient, then reads sender Held, B's public held record and same-ID replay. `online-held` unloads B and observes held custody with both Hosts online. Both stop before native input release. On the intermediate implementation these paths use its actual public Sent/Held DTOs and do not close final Push/layout or delivered-turn obligations. Keep `queue-*.json`, `central-sockets.json`, raw lifecycle evidence and `result.json`. A successful run requires no failure, no cleanup failures and zero actual native turn starts across every connection, including connections replaced during reopen. An automatic notification turn invalidates a no-model run even when no explicit `turn/start` was sent.

`online-timing` keeps both Hosts online, unloads B's recipient and sends twenty distinct queue items. It retains each actual sender outcome as a causal settlement barrier, then stops the owners and reads exactly one matching B inbox row per item. `timing-report.json` reports actual `accepted_at − created_at` samples and their median; no polling duration supplies a sample and no latency threshold is inferred. `timing-inbox-readback.json` preserves the original rows. Delivery/refusal outcomes remain alongside the timing report: an unloaded metadata-only native thread can return `recipientUnknown` after inbox acceptance. Such samples characterize inbox timing, not successful native delivery or a persisted recipient Session. Missing rows fail the drive. Final target qualification and staging remain separate obligations.

For read-only SQLite evidence after every consumer has settled:

```sh
node verification/run.ts --d1-readback "$D1_READBACK_PLAN"
```

The JSON follows `d1ReadbackSchema`: canonical `repository`, `databases` with explicit `fleet`/`sender`/`recipient` roles and canonical task-owned paths, exact UUIDv7 `itemIds`, and fresh task-owned `output`. The reader refuses an occupied or unverified database, retains full DDL/column/key/index definitions and selected mail metadata, and reports payload/body presence without copying message content. It explicitly does not prove an owning reopen. Join this evidence to the public mail ID and owner lifecycle evidence; absence without the correct holder/table and item is insufficient.

## Target and readiness

Obtain the Implementer's immutable batch declaration: full source and library revision, signed executable paths/hashes, conformance features/configuration, client, normal debug profile and deadline. Verify them before launch and after observation. At Central `ad34f727f`, the only physical roles are Fleet and project Copy: Fleet owns sessions/admission/outbox custody, pushes the exact item, accepts the recipient receipt and Report, then publishes status to the submitter. Earlier intermediate receipts do not qualify this target. Typed Submitter and D1 mail expiry/retention are pending their combined library/Central/Router pin; the deciding procedures below remain obligations, not claims that those paths are built or qualified.

Use three isolated debug Routers A/B/C on this computer and exactly two synthetic projects: P1 held by A, P2 by B. Each Router needs its own private Router root, isolated `CODEX_HOME`, ports, socket directory and actual installation identity under the declared worktree's `tmp/`. This D1 allocation is separately authorized; the ordinary recipes' normal-home guard does not implement it. Leave normal Codex/Router homes untouched. Recipients are real Luna Sessions; observe the declared model/provider/effort before input. The launch owner must declare how three native endpoints coexist, including short collaboration socket paths; separate Router roots alone do not establish native startup isolation.

The SSH-only pair setup and its legacy import/project allocation do not establish this topology. Reuse lifecycle components only where preconditions match. Keep raw observations/control acknowledgements under the commissioned private evidence root, outside every Cargo target.

Keep one setup across compatible observations, checking source/health before a case, after failure and after reopen. Readiness includes enrollment, actual Welcome, native availability, exact P1/P2 holders and owning Fleet socket inventory. A listening port alone is insufficient.

## Drives and deciding observations

- **Offline/held/online:** create B's recipient and retain its exact SessionRef; make it wakeable, then stop B's Router. From A send one queue-mode message. Retain A's accepted-by-Fleet status while B is offline. Start B without loading that recipient. Observe B's held inbox and A's in-inbox status, then open the same recipient and retain A's delivered status plus one correlated native input/output/ended turn. Read A/Fleet/B records and payload-release facts. A send receipt alone is insufficient.
- **Stores/connections:** enumerate actual owning stores after mail and require exactly Fleet/P1/P2. WAL/SHM sidecars are not stores. Require all three sockets on Fleet and none on other objects. Cloudflare bindings alone are not an instantiated-object inventory.
- **Crash/replay:** bind each cut to its exact item/frame, await the causal barrier, stop/reap that owner and reopen the same state. Cuts cover Fleet admission, Push result, Report, Status publication and B inbox acceptance. Read content custody and retained metadata/status debt. Foreign-machine and changed-content rejection leave the original unchanged. Handoff uncertainty ends Unknown without another native injection. Two forwarding origins need concurrently retained origin/holder rows, per-origin limits and late/wrong-holder replies. Object-dispatch holds cannot represent removed object boundaries.
- **Retention:** inject Router/central clocks while keeping authorization valid. At age 59 days require the original digest/header on A/Fleet/B; at day 60 require no matching record after each holder's due work settles. Submit otherwise valid 61-day-old items at each ingress and observe too-old refusal. `verification/observations/central/d1-mail-evidence.ts` checks all holders; released content is compatible with a retained day-59 header.
- **Project isolation:** enable persistent P1 failure and observe actual refusals before/during the case. While it remains active, exchange mail A↔B and read/write P2. Require P1 still failing before clearing the exact owned fault. One-shot CopyFull rollback does not satisfy this procedure.
- **Fleet schema:** read all table/column/key/index/constraint definitions from owning Tokio storage or the declared Cloudflare Fleet schema endpoint. Check prefixes, UUIDv7/timestamps, boolean-only CHECKs and each stated JSON exception against the table design. Keep the journal exception's owner disposition explicit. Schema-support reports are not DDL readback.
- **Local send:** two real Luna Sessions on A, one exact public message, one matching inbox record and no matching A outbox record. Correlate one native input, ignoring unrelated outbox rows. Check UUIDv7 and creation/holder times.
- **Online timing:** send twenty distinct items with A/B online. Retain actual B inbox rows and each `accepted_at − created_at` plus median. `verification/observations/central/d1-mail-evidence.ts` checks the view against UUID creation time. Keep original timestamp strings and millisecond measurements. No latency threshold is adopted; status polling and model completion are different measurements.
- **Enrollment negatives:** never-enrolled D attempts the actual central socket and is refused before upgrade, with unchanged Fleet item inventory. Separately enroll/revoke C, submit over the declared still-open session, and require sender-not-enrolled with unchanged Fleet outbox. Do not enroll D or bypass auth. The revocation/submit interleaving needs an actual declared control.

`verification/observations/central/d1-mail-evidence.ts` consumes normalized observation views, not a new product protocol. A live adapter must parse the real response, map fields explicitly and retain the complete original readback. Helper tests establish assertion sensitivity, not product behavior or adapter qualification.

The source-backed owner-client methods are `CentralOwner.projectFault(project, enabled)` (Copy `projectUnavailable` / `clearProjectFault`, requiring Unavailable/Idle readback) and `CentralOwner.fleetSchema()` (the actual Fleet schema endpoint). The schema reader preserves the raw reply, including fields it does not interpret, and rejects lease-support reports. These clients still need their declared real-target qualification before use earns O7/O8 credit.

## Staging-only declarations

**O5 reset (temporary):** await the Lead's live target/deploy/reset grant. Bind the staging script/object inventory, source, URL, receiver and relay. Seed higher prior observations and independent local content/outbox/history/identity. Reset only the granted target and re-enroll without changing configured identities. Await the partial-clear barrier, kill/reap there, restart and prove clearing precedes every central read. Require a valid new lease/project write and preserved local records. A local rehearsal cannot close staging cutover.

**O6 capacity (lasting):** await staging/metrics authority. Fix N, rate, duration and metric scope/window before running. Pin one Fleet object's actual identity/connections. Retain Cloudflare request rate, CPU per request and errors alongside Router push timings. Report the observed bound; invent no threshold or capacity beyond that workload. Tokio has no equivalent Cloudflare budget observation.

The current preparation entry cannot execute staging. Do not attach, enroll, wipe, deploy or query hosted analytics without the separate grant.

## Evidence and cleanup

Retain the target declaration, raw public responses, holder readbacks, exact IDs/digests, causal-control receipts, actual terminated/replacement process identities and per-check evidence paths. Missing checks stay open; a deciding failure fails its row. Exit zero from a preparation command gives no product credit.

Clear exact owned faults after failure too. Drain/reap only recorded debug children and verify sockets/publication gone. Read storage through its owner or after consumers settle. Preserve evidence after teardown and recheck normal configuration. Never restart production or remove deployed objects as cleanup. Return per-observation coverage and prerequisites; acceptance stays with the Lead.
