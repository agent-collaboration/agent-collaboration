# Worker startup and observer receipts

Use this split when the caller commissions local-polling Q5. Q5(a) observes the converted workers through a fixture-only Host; it does not start native Codex, real Claude or Cursor. Q5(b), a real native observer connection, stays unverified until the caller supplies a route that excludes existing native account credentials.

The adopted local-polling Verification Design's Q5 requires owed work from a stopped process to run at startup. An empty-root first-pass/idle observation is the narrower, explicitly commissioned Q5(a) split; it does not by itself establish retained owed-work recovery.

## Fixture-only worker route

The entry is `verification/observations/runtime/worker-startup.ts`. Its `--help` and `workerStartupPlanSchema` describe the inputs. Run from the declared repository root. The caller supplies the exact frozen product commit, permitted ports, signed artifact qualification, a fresh private runtime directory and a fresh empty private evidence directory. Keep evidence outside runtime storage and every Cargo target.

```sh
node verification/observations/runtime/worker-startup.ts --help
node verification/observations/runtime/worker-startup.ts "$WORKER_STARTUP_PLAN"
```

The plan fields are `repository`, `source_head`, `qualification`, `run_directory`, `evidence`, `router_port`, `mcp_port`, `expected_version`, `expected_workers`, `idle_milliseconds` (default 5000), and timezone-aware `deadline`. `run_directory` must already be an empty canonical owner-private direct child of `/tmp` (normally `/private/tmp` on macOS), with at most 32 UTF-8 bytes so the example's nested provider sockets fit macOS's Unix socket limit. Evidence must be a separate private temporary directory. Do not repurpose a root that contains earlier runtime data.

Qualification uses the existing binary-record shape: `binaries` entries have `target`, `path`, `sha256`, and `testGuardFeatures`.

| Artifact | Required binding |
| --- | --- |
| `codex-router` | This checkout's `target/debug/codex-router`, strict signature, `dev.shravansunder.codex-router.debug`, TeamIdentifier `974QD84WVC`, empty test guards |
| `provider-only-debug-host` | This checkout's `target/debug/examples/provider-only-debug-host`, strict signature, `dev.shravansunder.test.provider-only-debug-host`, same team; retain its actual Cargo feature list |

This proof example is declared apparatus. Its dev-dependency feature closure can include runtime/test support; keep that classification in evidence instead of presenting it as a feature-free shipping Host. Do not weaken the normal Router's qualification to accommodate the example.

Build one package at a time, from the frozen checkout, in its normal target directory:

```sh
env -u CODEX_ROUTER_HOST_INHERITED_LOCK cargo build -p codex-router-cli --bin codex-router
env -u CODEX_ROUTER_HOST_INHERITED_LOCK cargo build -p codex-router-host --example provider-only-debug-host
scripts/cargo_debug_signing_runner.sh --sign-only target/debug/codex-router target/debug/examples/provider-only-debug-host
```

Check the available disk before each build and retain the caller's floor. The signing runner may use the approved signing identity; a missing identity, locked signing access or owner prompt is a stop, not permission to run unsigned. The live driver independently verifies signatures and hashes before either Rust executable starts.

The driver creates an executable wrapper around the repository's `provider-startup-idle-agent.ts`, using the current absolute Node interpreter. The interpreter path must contain no whitespace. A private PATH contains only a `claude` alias to that wrapper, so the Host's metadata binding cannot resolve the installed real Claude. The fixture accepts `initialize` and rejects any subsequent session/model request. Its receipt records only initialization, PID and request counts, never request bodies.

The Router uses only `run_directory/router` and its newly provisioned local token, with the existing plaintext-debug flag. The provider-only example uses a separate task-owned empty catalog and peer registry; it starts no native process and never calls `backend_ready`. Before publishing readiness, the example must configure provider operation retention through `configure_provider_operation_retention(None, SystemClock)`, as the normal Host lifecycle does. A component runtime start alone does not start that worker. No `HOME` or `CODEX_HOME` redirection is performed. This is the explicitly commissioned fixture route, not normal Codex session-inventory proof.

## Worker evidence

Require the declared source to have the example's stderr fmt subscriber and ordinary operational events on target `worker_loop`. Set `RUST_LOG=worker_loop=debug`; the driver does this for the owned example only. The source currently emits `pass started` with `worker`, and `sleeping` with `worker` and `armed=None` or `Some(unix_millis)`. Qualify the actual frozen source before using these selectors. The owners are the [loop helper](../../../../crates/collaboration-runtime/src/wake_driven_worker_loop.rs), [Host worker construction](../../../../crates/codex-router-host/src/collaboration_runtime/startup_composition.rs), and [provider-only example](../../../../crates/codex-router-host/examples/provider-only-debug-host.rs).

Declare every worker being claimed. With the current private stores opening successfully, the provider-only inventory names `schedules`, `wake-ups`, `push hand-off`, `automation_retention`, `provider_operation_retention`, and `fleet_outbox_retention`. Provider retention requires the explicit configuration call above. Fleet retention requires a composed local board with a Fleet client; `board_session: None` omits the machine session but still opens that local composition, so it does not exclude the Fleet worker. Qualify those conditions on each frozen source. A missing worker or current pass without a following sleep cannot pass. A deadline inside the proposed idle window leaves that window unsuitable for idle proof.

`startup.json` retains the actual Host identity/publication, fixture receipt, and worker events. `idle.json` requires unchanged worker-loop events over the measured window. This establishes no additional passes in that window. The corresponding absence of store reads for empty-root workers depends on the inspected source path and absence of in-flight units; the driver does not expose a direct SQLite IO counter. Keep that distinction explicit. It does not establish indefinite quiescence or genuine provider cessation.

`native-unavailable.json` reads only this root's backend/observer rows and rejects a native-ready fact on this no-native route. Real observer connection remains unverified here.

After the window, the driver settles only its recorded Host, fixture descendants and Router using fresh PID/UID/birth/executable checks. Inspect `worker-startup-result.json`: failure must be null, Host must exit 0, Router must exit 0 or terminate from SIGTERM recorded as sent to its exact PID, no cleanup errors or owned processes may remain, and evidence must survive. The standalone Router serve command has no signal shutdown handler; its actual signal exit remains in the receipt. An interrupted or failed run still keeps its partial evidence and cleanup result. Never signal by process name or remove another root.

## Observer reader and normal owned-runtime preparation

`startup-observation.ts` opens only the selected root's `agent-communication/session-registry.sqlite` read-only. It selects backend facts, preserves exact sequence integers, and requires the latest backend-ready and coverage-restored facts to match the published service epoch and native generation. Backend and observer IDs differ legitimately. Stale epochs/generations, later coverage loss, missing journals and invalid stored facts cannot establish connection. It never reads thread rows into evidence.

The [normal Host startup](../../../../crates/codex-router-host/src/lifecycle_owner.rs) awaits native app-server readiness before constructing collaboration workers. An unusable managed executable fails earlier. Native startup can load persistent account credentials eagerly; the [debug profile projection](../../../../crates/codex-native-integration/src/debug_profile_projection.rs) currently cannot project an ephemeral auth-store override. The caller owns that access decision. Do not redirect Codex home, modify its saved files, or infer that selecting a debug model provider excludes native auth.

## Apparatus checks

Run the package's `check`, `lint`, `format:check`, and `test` scripts. Fixture lifecycle tests launch only recorded harmless Node children and disposable databases. Their pass is apparatus qualification, not the Q5 runtime observation. Keep live results, source/artifact hashes and open obligations in the task evidence, outside this recipe.
