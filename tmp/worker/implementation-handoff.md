# W1 implementation handoff

Date: 2026-10-29. Stage: in-progress.

Repository: `/Users/shravansunder/code/projects/codex-router.test-no-wall-clock-host`.
Branch: `test/no-wall-clock-host`; base `881f778c77b4d5532ef1be32f0bd2f44daaf73f5` (0.1.116).
Head: the WIP checkpoint containing this file; the exact SHA is in the Lead handback and `git rev-parse HEAD`.
Plan identity: none. Governing authority is the owner's W1 packet plus the Lead decisions recorded in `trace.md`. No independent review has occurred. The Lead will commission a reviewer after completed implementation and proof.

The current authority is Lead push `01a18736-0175-7650-a32e-ccbb6fd226a0`: stop local builds/tests, push WIP, continue all validation on Sunclaw. No further Cargo, nextest, or pnpm test commands on this Mac.

## Partial changes

These are source changes, not verified conversions:

- `AGENTS.md` contains exactly the requested testing rule.
- `collaboration-runtime` wake worker/sender accept the existing `runtime_primitives::Clock`; scheduling, claim, push settlement, and completion read it. The worker has a test-support-only watch reporting active dispatch count at owner checkpoints.
- `collaboration-service` and Host composition adapt the SAME SubscriptionClock instance. The old private MailClock adapter was moved to `SubscriptionRuntimeClock` and shared with both mail and wake owners; no reverse Cargo edge was added.
- The recurring native-wake test now pauses Tokio, explicitly advances UTC, retains both native queue deliveries, waits for committed first completion before advancing 299+1 seconds, and asserts no early second allocation. Its new permanent support module prevents auto-advancing through real IO and advances worker ticks only between dispatches.
- Remote-wake replay tests set their existing injected recipient clock after the frozen deadline instead of sleeping through it.
- MCP test waits observe registry activity. The shutdown-grace check observes actual shutdown-token processing, then checks that the registry future remains pending. Negative response delay in the abandon helper is replaced by a pending read at admission.

## Proof and limits

- Initial baseline invocation failed with exit 101 before compilation: a qualified CLI feature was requested without selecting the CLI package. The correction omitted that feature because no CLI package was selected.
- Corrected baseline command: `/usr/bin/time -p cargo nextest run --profile ci --all-features -p acp-client-runtime -p agent-collaboration -p board-legacy-upgrade -p codex-router-host -p codex-router-secret-store -p codex-router-state -p collaboration-central-conformance -p collaboration-mcp -p collaboration-runtime -p collaboration-service -p codex-router-tailnet-verification-host`.
- Old-source compilation finished in 5m44s. Binary signing/listing was still running when the owner stopped local test work. SIGINT exit 130; elapsed command wall time 1087.41 s. **No test results, no pass/fail counts, no valid before-test total.** Raw output: `before.log`.
- The changed source has not been compiled, formatted, linted, tested, or mutation-checked. No runtime claim or sensitivity claim is made. No release check, module-cycle check, version bump, PR, or CI run occurred.
- The broad inventory records 2089 initial timing/timestamp occurrences grouped into 833 rows, including production and timestamp-only context. The explicit reviewed entries at its top name conversion plans, but ambiguous scan entries remain. **The inventory classification is incomplete.**

## Remaining work

1. Refine every inventory entry into actual A/B/C or timestamp/product-only context; retain the explicit real-staging JWT exception X. Include hidden duration behaviour driven through public timeout/schedule APIs, not only literal sleep calls.
2. Compile and validate the current WIP on Sunclaw first. Review the new worker observation/paused-IO driver for hangs, wrong observer ordering, and unchanged real protocol assertions. Validate MCP cancellation/drop observation paths. These are untested and may need correction.
3. Diagnose CI16 from its real owner path; the current test already pauses time and still waits on an intermediate DueRoots checkpoint. No root cause has been established or repair made.
4. Finish all other A/B conversions in `scope.txt`, including ignored/manual journeys; the work remains much broader than this partial diff. CI6/CI9/CI10 status needs current-source inspection. STG-H1/H2 live in W2-owned verification TypeScript.
5. Implement local Access expiry with the existing edge clock. Proposed local helper signature: `run_access_expiry_conformance(client, now: chrono::DateTime<chrono::Utc>, advance_clock: impl FnOnce(DateTime<Utc>) -> Future<Output = ConformanceResult<()>>)`. The callback receives exp+5 seconds after initial acceptance, preserving identical signed bytes and real HTTP/crypto. No signature change exists yet.
6. Resolve mechanical edits/integration of the two W2-owned callers through the Lead. Tokio already has `tests/support/clocked_server.rs`. Local workerd hardwires CloudflareClock; the Lead permits a local-only compile-time feature/control only when deployed artifacts cannot contain it and a check rejects deployed builds with it. No new runtime auth control is authorized in normal builds. Inspect `.github/workflows/ci.yml` local versus deployed worker-build commands and Cloudflare deployment docs. No feature/control/deploy check has been implemented.
7. Run at least three deliberate product mutation controls with intended failures and restored greens. Then fmt; touched-package all-target/all-feature Clippy with `-D warnings`; touched-package nextest with `--profile ci`; CLI feature support when selecting CLI; locked release check for the three required bin packages; module-cycle ratchet. Measure comparable before/after test durations on Sunclaw, since the local baseline is incomplete.
8. Merge origin/main before final push; use assigned W1 version 0.1.117 only if still next unused at merge time (Lead owns assignment). One PR to main, no merge authority. Wait for CI with the requested 240-second watch interval, then Lead handback/review.

## Boundaries and risk

Do not touch proxy, production Router/Host, real Codex home, upstream forks/pins, or W2 files without Lead authorization. No Python, lint allows, timeout increases, removed assertions, or added retries. No file over 1000 lines. Commit unsigned, no stash/amend/no-verify. One Cargo command at a time, on Sunclaw only under the newer instruction.

The clock adapter changes an existing public constructor as specifically approved by the Lead; all known in-scope callers were updated, but compilation is unobserved. No persisted format, remote request, security control, or deployed clock behaviour was changed. Merging this WIP today risks compile/lint failures and test deadlocks; it is not merge-ready.

This task's generated local target was cleaned after stopping all its consumers; baseline evidence is retained here. No running production process was stopped or replaced.

Recommended next action: continue through a Lead-authorized Sunclaw executor using `copy-paste-prompt.md`, not implementation acceptance.
