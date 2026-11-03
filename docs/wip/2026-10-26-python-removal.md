# Python removal: disposition and proof

Base: `be7e61e8bca32ebac172eb743e8064d2c7d6ffe7`. Inventory: 61 tracked files; 43 originals replaced, 7 unused files deleted, and 11 removed by merged PR #161. Zero tracked Python files remain. Embedded Rust/shell programs and active instructions are included. PR #165 is merged and the workflow cutover is implemented on top of main; final-head CI and release dry-run results are reported in PR #169. This report does not claim merge readiness.

This is the direct unpaired Worker assignment. The Lead verifies the diff/logs and arranges independent review before merge. The TUI import closure was preserved until #161 merged as `6388b973`. That PR removed its 11 entrypoints/tests; this PR then removed the six orphaned Python support modules after a tracked reference search. Their TypeScript owners remain active. No production Router, tag, actual Homebrew/tap operation, provider model or deployment was touched. The workspace inherits main’s0.1.105; the next version remains the Lead’s merge-time assignment.

## Per-file dispositions

Every original file has exactly one disposition. Proof letters are explained below; adjacent replacement modules split responsibilities where needed.

| Original | Disposition | Replacement / proof |
| --- | --- | --- |
| `crates/acp-client-runtime/tests/cursor_model_settings/scripted_acp_agent.py` | Ported | `verification/fixtures/cursor-settings-agent.ts` (F) |
| `crates/codex-router-host/src/external_provider_runtime/acp_scripted_fixture.py` | Ported | `verification/fixtures/scripted-acp-agent.ts` (F) |
| `crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/fixed_quota_child.py` | Removed by #161 | Merged in main at `6388b973` |
| `crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/fixed_ttyd_adapter.py` | Removed by #161 | Merged in main at `6388b973` |
| `scripts/check-http-streaming-guard.py` | Ported | `verification/tooling/check-http-streaming-guard.ts` (G) |
| `scripts/check-proxy-db-runtime-isolation.py` | Deleted | D |
| `scripts/check-release-runtime-guardrails.py` | Ported | `verification/tooling/check-release-runtime-guardrails.ts` (G) |
| `scripts/proof-tools/socket-permission-probe.py` | Ported | `verification/tooling/socket-permission-probe.ts` (N) |
| `scripts/publish_homebrew_tap_local.py` | Ported | `verification/tooling/publish-homebrew-tap-local.ts` (H) |
| `scripts/tests/test_acp_metadata_observation.py` | Ported | `verification/observations/acp-metadata.test.ts` (M) |
| `scripts/tests/test_bootstrap_tools.py` | Ported | `verification/tooling/bootstrap-tools.test.ts` (C) |
| `scripts/tests/test_prepare_sqlx.py` | Ported | `verification/tooling/sqlx-preparation.test.ts` (C) |
| `scripts/tests/test_prepare_sqlx_turso.py` | Ported | `verification/tooling/sqlx-preparation.test.ts` (C) |
| `scripts/tests/test_publish_homebrew_tap_local.py` | Ported | `verification/tooling/release-command-process.test.ts` (H) |
| `scripts/tests/test_rust_file_size.py` | Ported | `verification/tooling/rust-file-size.test.ts` (C) |
| `scripts/tests/test_tailnet_mcp_https_transport.py` | Ported | `verification/observations/tailnet-curl-transport.test.ts` (T) |
| `scripts/tests/test_tailnet_mcp_verification.py` | Ported | `verification/observations/tailnet-oracles.test.ts` (T) |
| `scripts/tests/test_tui_capture.py` | Removed by #161 | Merged in main at `6388b973` |
| `scripts/tests/test_tui_component_capture.py` | Removed by #161 | Merged in main at `6388b973` |
| `scripts/tests/test_tui_public_sessions.py` | Removed by #161 | Merged in main at `6388b973` |
| `scripts/tests/test_tui_verification.py` | Removed by #161 | Merged in main at `6388b973` |
| `scripts/tests/test_update_homebrew_formula.py` | Ported | `verification/tooling/release-tooling.test.ts` (H) |
| `scripts/tests/test_verification_lifecycle.py` | Ported | `verification/observations/lifecycle.test.ts` (L) |
| `scripts/tests/test_verification_observations.py` | Ported | `verification/observations/observation-core.test.ts` (O) |
| `scripts/tests/verification_controller_fixture.py` | Ported | `verification/observations/lifecycle-fixture.ts` (L) |
| `scripts/tooling/check-central-worker-bundle.py` | Ported | `verification/tooling/check-central-worker-bundle.ts` (C) |
| `scripts/tooling/check-rust-file-size.py` | Ported | `verification/tooling/check-rust-file-size.ts` (C) |
| `scripts/tooling/prepare-sqlx-turso.py` | Ported | `verification/tooling/prepare-sqlx-turso.ts` (C) |
| `scripts/tooling/prepare-sqlx.py` | Ported | `verification/tooling/prepare-sqlx.ts` (C) |
| `scripts/tooling/sqlx-package-rustc-wrapper.py` | Ported | `verification/tooling/sqlx-package-rustc-wrapper.ts` (C) |
| `scripts/tooling/sqlx-sqlite-rustc-wrapper.py` | Ported | `verification/tooling/sqlx-sqlite-rustc-wrapper.ts` (C) |
| `scripts/tooling/verify-sqlx-contract.py` | Deleted | D |
| `scripts/update_homebrew_formula.py` | Ported | `verification/tooling/update-homebrew-formula.ts` (H) |
| `scripts/validate-proxy-db-runtime-isolation-artifact.py` | Ported | `verification/tooling/validate-proxy-db-runtime-isolation-artifact.ts` (G) |
| `scripts/verification/acp_metadata_observation.py` | Ported | `verification/observations/acp-metadata-observation.ts` (M) |
| `scripts/verification/board_journey_peer.py` | Deleted | D |
| `scripts/verification/board_journey_runtime.py` | Deleted | D |
| `scripts/verification/board_journey_target.py` | Deleted | D |
| `scripts/verification/board_journey_values.py` | Deleted | D |
| `scripts/verification/config_sections.py` | Ported | `verification/observations/config-sections.ts` (O); orphaned Python original removed here after #161 |
| `scripts/verification/harness_report.py` | Ported | `verification/observations/harness-report.ts` (O) |
| `scripts/verification/interaction_journey.py` | Ported | `verification/observations/interaction-journey.ts` (I) |
| `scripts/verification/interaction_readback.py` | Ported | `verification/observations/interaction-readback.ts` (I) |
| `scripts/verification/mail_journey_peer.py` | Deleted | D |
| `scripts/verification/native_correlation.py` | Ported | `verification/observations/native-correlation.ts` (O) |
| `scripts/verification/owned_runtime.py` | Ported | `verification/observations/owned-runtime.ts` (L); orphaned Python original removed here after #161 |
| `scripts/verification/process_inventory.py` | Ported | `verification/observations/process-inventory.ts` (L) |
| `scripts/verification/process_ownership.py` | Ported | `verification/observations/process-ownership.ts` (L); orphaned Python original removed here after #161 |
| `scripts/verification/provider_session_control.py` | Ported | `verification/observations/provider-session-control.ts` (I) |
| `scripts/verification/readonly_codex_home.py` | Ported | `verification/observations/readonly-codex-home.ts` (O); orphaned Python original removed here after #161 |
| `scripts/verification/runtime_qualification.py` | Ported | `verification/observations/runtime-qualification.ts` (L); orphaned Python original removed here after #161 |
| `scripts/verification/tailnet_mcp.py` | Ported | `verification/observations/tailnet-mcp.ts` (T) |
| `scripts/verification/tailnet_mcp_client.py` | Ported | `verification/observations/tailnet-transport.ts` (T) |
| `scripts/verification/tailnet_mcp_host.py` | Ported | `crates/tailnet-verification-host/src/main.rs` (T) |
| `scripts/verification/tailnet_mcp_models.py` | Ported | `verification/observations/tailnet-plans.ts` (T) |
| `scripts/verification/tui_capture.py` | Removed by #161 | Merged in main at `6388b973` |
| `scripts/verification/tui_capture_lifecycle.py` | Removed by #161 | Merged in main at `6388b973` |
| `scripts/verification/tui_component_capture.py` | Removed by #161 | Merged in main at `6388b973` |
| `scripts/verification/tui_public_sessions.py` | Removed by #161 | Merged in main at `6388b973` |
| `scripts/verification/tui_verification.py` | Removed by #161 | Merged in main at `6388b973` |
| `scripts/verification/typed_observations.py` | Ported | `verification/observations/evidence-models.ts` (O); orphaned Python original removed here after #161 |

## Original-input proof

Golden inputs/results come from the base implementations. Fixture data retains source hashes. Temporary paths and process IDs are normalized only where declared; PID receipts are checked against the actual spawned child before normalization. Language stack traces can differ; protocol, receipt, formula and guard outputs are checked at their deciding boundaries.

| Proof | Comparisons and retained evidence |
| --- | --- |
| H: Homebrew | Captured releasev0.1.102, asset SHA256 `54e978fb41bab35653a998ffdc50a9f4c49e83240fe5458565072e575d4c1517`. Original/new formula CLIs produce byte-identical files, stdout and stderr; CRLF behavior also matches. The whole local publisher CLI runs against identical fake external commands: formula, command trace and stdout match after declared temporary-path normalization. Permanent tests: `verification/tooling/release-tooling.test.ts` and `release-command-process.test.ts`. No real brew, tap or publication ran. |
| C: CI tooling |51 original Worker bundle exit/output cases;14 original SQLx preparation scenarios compare commands, environment, refusal/commit behavior and cache bytes;8 rustc-wrapper environment vectors plus executable-process checks. Rust size checks cover physical bytes, Git inventories, quoted paths,6 source-parser fixtures and9 line-count vectors. Bootstrap tests preserve successful/stale/failed/offline/check-mode outcomes with controlled installers. Tests are under `verification/tooling/tests/`. |
| G: guards/artifacts | Original HTTP G24/G25 outputs match exactly. Release guard rows retain original results with timestamps normalized, including pre-existing failures. Proxy DB artifact fixtures retain root, pressure, ordering, correlation and privacy checks. See `verification/tooling/structural-guards.test.ts` and its original fixture data. |
| O: observations | Configuration sections, evidence models/JSON, reports, native read protocol and read-only Codex-home checks use frozen original observations, including unknown fields, exact integers and invalid inputs. Tests: `verification/observations/tests/{observation-core,config-sections,harness-report}.test.ts`. Raw validated records remain the serialization boundary where the original omitted unset fields. |
| L: lifecycle |19 original scoped lifecycle outcomes plus real harmless children cover identity retention, unknown inventory, deadlines, cleanup and no signal on mismatch. Signed macOS UIDs and child reaping are checked. See `verification/observations/lifecycle.test.ts` and the original lifecycle baselines. |
| M: metadata | Original oracle cases plus an owned Unix WebSocket check target bindings, metadata/prose separation, bounds and refusal controls. Tests: `verification/observations/acp-metadata.test.ts` and `acp-metadata-transport.test.ts`. This adds no installed-provider/TUI observation. |
| I: callbacks |26 original oracle comparisons plus real local HTTP/SQLite fixtures exercise create/control, decisions, retention, independent readback and the complete journey. Tests: `verification/observations/tests/{callback-oracles,provider-control,callback-journey}.test.ts`. |
| T: Tailnet | Entire Host driver is Rust by the owner's relayed decision. Original profile/observation/URL fixtures and real owned process/socket tests retain no-follow, nonblocking lock, owner/mode, inode/device, byte guards and exact restoration. TS peer tests retain curl/system trust, bounds and independent Unix/SQLite readback. See `crates/tailnet-verification-host/tests/` and `verification/observations/tests/tailnet-*.test.ts`. No real Host/profile/Tailscale/two-machine run occurred. |
| F: executable fixtures | Shared ACP settings/scripted agents retain original request/response bytes and receipts. Broader migration:173 frozen ACP/CLI scenarios,3 ordered socket transcripts,12 restricted-PATH native executable cases and6 original shell-word vectors under `verification/fixtures/tests/`. The full oversized initialize fixture retains67,108,866 original output bytes and the actual child PID. |
| N: permission probes | Owned live Unix/TCP listeners ensure allowed connections cannot produce denial receipts; only EPERM/EACCES counts as denial. Old/new embedded probes match exit/stdout and permission classification on the same allowed/denied Unix inputs. See `verification/tooling/socket-permission-probe.test.ts`. Complete native Seatbelt/delivery acceptance remains unrun. |

The original captured-release receipts are `tmp/python-removal-proof/final-formula-comparison.json`, `formula-newline-comparisons.json` and `local-publisher-comparison.json`. Later published-release refreshes are identified in the PR handback. The publisher proof substitutes every external command and retains the resulting formula; it does not establish an actual Homebrew installation or tap publication.

## Embedded programs and consumers

| Surface | Replacement and proof |
| --- | --- |
| ACP/Host provider strings | Focused TS agents and shared stdio/socket/pipe helpers. Original/replacement ACP suites both83 pass. Host runtime suites both69 pass/3 ignored; broader provider owners both127 pass/3 ignored.173 original wire/receipt cases and3 socket transcripts pass. |
| Native Claude executable/catalogue/adapter fixtures | Absolute-Node executable wrappers preserve restricted PATH.12 byte/receipt cases; original/replacement native job6, global harness3, service catalogue3, Host catalogue1 plus its controlled child, CLI background9. Current native catalogue publication6 passes. |
| Tailscale test commands and leases | TS preflight/lease agents. Original/replacement24 tests and2 exposure-startup tests pass. Real Tailscale state is untouched. |
| Host supervisor and composed routes | Focused TS agents and one launch-helper module per test target. Original/replacement supervisor13, delivery/schedule/steering/race29 and FIFO/message-route7 pass, plus composed routes. Final shared-helper consumers13 pass. |
| SSH carriers |10 original native/managed receipt cases, one publication wire case and6 original shlex vectors. The permanent Rust parent checks all10 scenarios and no replay of uncertain submissions. |
| Startup token/exec wrapper | `token-observed-exec.ts` preserves the original token-file bytes, argument vector, environment, inherited stdin and process PID through native exec. Three original-wrapper comparisons and the real startup consumer pass. The first CI run found this previously missed plain quoted program; the expanded string/command audit covers that form too. |
| Oversized initialize fixture | The raw-byte failure path uses built-ins and one byte buffer, retaining the original full frame and completed flush. JSON setup stays on the EOF scenario. The original/replacement64 MiB output hashes and PID receipts match; all16 admission tests pass. Frame size, two-second runtime deadline, typed rejection and process-reaping assertions remain intact. |
| SQLite/stderr pressure | Same disposable DB retains the same rows, blocks a writer after `acquired`, and releases on exit. The installed harness's ordinary unit tests pass17;11 pre-existing opt-in cases stay ignored. |
| Shell path guards |64 original CLI observations cover missing paths, links, loops and parent traversal. Two regressions fail the earlier incorrect port and pass the original/current implementation. |
| Proof matrix heredocs |79 original passed/refused receipt cases compare bytes/messages. Touched-target metadata deliberately points to the new TS guard. Source freshness guards include moved implementation/dependencies. |
| Quota smoke assertions/network |208 original acceptance/refusal observations, original handshake/masked frame, fixed-mask bytes and listener-regex observations pass. Full installed-Codex smoke remains unrun. |
| Public UUIDv7 hint | Lead-approved text: “Generate a fresh UUIDv7 and pass it as operationId.” Field/error/validation contract retained. Public MCP conversation9 and diagnostic2 tests pass. |
| Active docs | Existing Rust E2E/conformance and TS runtime/import entries replace retired commands, as confirmed by the Lead. Governing obligations remain; unmapped routes stay open. #161 TUI commands are untouched. |

## Deletion reference searches

For each name, run `git grep -n -F -e NAME be7e61e8 --`. Full results are retained in `tmp/python-removal-proof/deletion-reference-search.json`.

| Name | Base results and disposition reason |
| --- | --- |
| `check-proxy-db-runtime-isolation` | Zero references. Unused wrapper; live artifact validator is ported separately. |
| `verify-sqlx-contract` | One historical design comment; no executable consumer. Current SQLx preparation/wrappers are ported. |
| `board_journey_peer` | Three: retired README and two references in the dead mail helper. |
| `board_journey_runtime` | One, inside the dead mail helper. |
| `board_journey_target` | One, inside the dead board runtime helper. |
| `board_journey_values` | Five, inside the dead closure and its isolated test import. |
| `mail_journey_peer` | Two, both in the isolated dead-peer test. |

The five board/mail helpers form an unreachable closure after their journey entry retired. The isolated dead-peer receipt test is removed with that closure; live observation tests are ported. Current board/upgrade/central Rust journeys and the TS runtime harness remain. Historical specs and original-proof metadata are evidence, not executable consumers.

## Local checks and remaining gates

- `pnpm --dir verification test`:835 passed,0 failed, exit0. Lifecycle tests need process inspection of their own harmless children; a restricted-sandbox `/bin/ps` refusal is not a passing run.
- Type check, lint and formatter pass. Focused fixture-cutover rerun:91 passed,0 failed.
- `cargo clippy -p acp-client-runtime -p agent-collaboration -p codex-router-host -p codex-router-test-support -p collaboration-mcp -p codex-router-tailnet-verification-host --all-targets -- -D warnings`: exit0 with narrow debug profiles.
- Cargo formatting, Git whitespace and the Rust1000-line guard pass. Existing generated Cargo.lock/pnpm-lock.yaml exceed1000 lines; authored files remain within the cap.
- Main through6388b973 is merged without rebase. The four validation jobs, background/wait-all structure and main-only cache policy are preserved. Static validation runs the TypeScript suite; Rust fixture consumers install pinned Node/dependencies, and SQLx/size/Worker checks call the ports. The release dry run generates an idempotent formula preview from its packaged digest and retains it while publication/tap steps remain disabled. Final-head CI and release dry-run links/results belong to PR #169 and the handback.
- Version bump awaits the Lead's merge-time reservation. Independent review and merge remain outside this Worker's acceptance authority.
- Fake-command/apparatus evidence does not establish real Homebrew publication, provider behavior, hosted delivery, production replacement or a real Tailnet journey.

The first workflow-cutover CI at9a0600ae passed5568 workspace cases but found the startup wrapper above and timed out one unchanged subscription-view test. The wrapper is corrected with original-input proof. The unchanged test passes on merged main in2.24s and locally in1.32s; the cause of its300s CI timeout remains unconfirmed, and no timeout, scheduler, test-selection or product behavior was changed. Corrected final-head CI/release results are linked from PR #169.

The next CI atb4afbf06 passed5569 workspace cases, including both tests above, and failed the oversized-initialize assertion after2.088s. Its diagnostic omitted the actual error variant. The fixture now avoids shared JSON startup and a large string conversion; an added permanent original-output comparison covers the complete64 MiB frame. Local byte/receipt parity, the unchanged rejection/reaping test and all16 admission tests pass. The assertion now prints any unexpected error variant. Refreshed Linux CI remains the required proof for this correction.

## Accepted review corrections

- SIGINT uses the existing native interruption flag and a scoped TypeScript listener. Both wait for owned-child settlement; the native profile restore remains in the same Rust-owned critical section. A private profile wrapper shared with Host execution makes the real-signal test exercise the production restore operation. The TypeScript test checks child reaping at settlement before fixture fallback cleanup.
- Real-signal subprocess tests fail without the wiring and pass with it. The Rust negative control observes both an unreaped harmless child and projected disposable profile bytes. Green runs require child absence, settled evidence and exact original profile bytes; no production profile, Host or Tailscale instance is used.
- Before #161 merged, all 17 retained Python files matched main in bytes and mode. Nine module --help checks, a stdlib quota-child import, adapter --version dispatch to a controlled stand-in, and 45 retained unit/fixture tests passed. This historical import/dispatch proof closed the original review finding. The later Lead instruction supersedes retention: #161 removed 11 files, and the six orphaned support originals are now removed here.
- `.sqlx/README.md`, `docs/testing/rust-reliability-tools.md` and the central Worker README now use the TypeScript commands and the Node/pnpm pins in `verification/package.json`. Historical receipts remain unchanged.
- Pre-integration local quality: 835 TypeScript tests passed; the complete Rust driver suite passed 14 tests (its one ignored subprocess fixture is invoked by the passing parent signal test), and its all-target Clippy check with `-D warnings` passed. Fresh final-head CI, including the explicit private-root correction below, and release dry-run results are recorded in the PR and handback.

## Final closure after #161

A tracked search for `process_ownership|owned_runtime|typed_observations|readonly_codex_home|runtime_qualification|config_sections` found 22 lines: 12 inside the six-module Python closure, six report provenance rows, and four unrelated TypeScript schema/property uses. There were no outside executable consumers. Raw results are `tmp/python-removal-proof/closure-removal-all-tracked-references.txt`. Removing the six modules makes `git ls-files '*.py'` empty; the embedded-program audit contains only explanatory comments.

The workflow merge retains both TypeScript packages and their declared Node versions, #161's qualified native adapter setup, four validation jobs, background/wait-all steps and main-only caches. No Python checks remain. The release job retains the owner's 45-minute limit.

The native real-signal test now requests a 0700 temporary root explicitly. Its child private-root guard, signal/child/profile assertions and deadlines remain intact. Hosted workspace tests explicitly use umask022; the earlier unwired negative-control receipts remain preserved. No local Rust executable/test was launched after the owner's stop. Final-head CI and signed release/formula proof are required in the PR handback.
