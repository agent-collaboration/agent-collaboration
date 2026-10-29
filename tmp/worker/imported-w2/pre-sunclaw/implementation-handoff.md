# W2 implementation handoff

Date: 2026-10-29. Stage: in-progress, stopped locally by owner.
Branch: test/no-wall-clock-cli. Base: 881f778c77b4d5532ef1be32f0bd2f44daaf73f5 (main 0.1.116). Head: the commit containing this file. No PR.
Plan identity: none. Authority: packet.md plus Lead decisions below. No independent review.

All builds/tests move to Sunclaw. Do not run cargo, builds, nextest or pnpm tests on this Mac. Lead stop 01a18736-28dd-77e3-b901-bd0e45f3d844 requires an unsigned WIP checkpoint/push. The original completion boundary remains future work.

## Changes written

All are unverified candidates:

- crates/collaboration-client/src/conversation_client_tests.rs: provider-create wait uses start_paused; zero delay bypasses sleep.
- crates/codex-router-quota-tui/src/quota_browse_rendering_test.rs: paused timer and actual beta frame before exit.
- crates/codex-router-quota-tui/src/quota_floor_editor_presentation_test.rs: first floor save sends events after rendered prerequisites; focused-account/saved 11-percent assertions retained.
- crates/codex-router-quota-tui/src/quota_frame_observation.rs: helper reuses next_matching_frame, input channel and bounded loop completion. Source-only ElementExt import correction after stop; no compilation/formatter.
- scripts/verification/tui/driver-lifecycle.test.ts and public-lifecycle.test.ts: fake deadlines, fs.watch plus initial-state readiness, explicit child release and real child completion. Last run failed; inspect fake-timer/real-I/O interaction before trusting these.

No production behavior, schema, public API or dependency changed. No proxy or W1 source changed.

## Proof and limits

- inventory.md: 1,132 real-time API sites, 434 grouped rows, recorded before edits. This is scan coverage with plans; timestamp/guard groups still require individual oracle review.
- baseline.log: original Rust sources compiled with nextest --profile ci --all-features --features codex-router-cli/keychain-test-support and all 14 W2 packages. Build 5m50s. Owner stop sent SIGINT to exact PID 56771; exit 100. 471/1858 run, 447 passed, 24 failed including 16 interrupt settlements, 51 skipped, 1387 unrun. Runtime 268.765s, command wall 1053.73s. Not a complete baseline or speed comparison.
- typescript-before-unsandboxed.log: Node v26.12.1, 150 passed / three failed, 39.97s suite / 41.45s command, exit 1. Two late-oracle deadline failures; one collection timeout.
- typescript-converted-focused.log: 133 passed / 20 failed, 102.33s, exit 1. Attempted filename filter still ran all 153 tests. Child-readiness/lifecycle waits failed; diagnose on Sunclaw. No green claim.
- Initial Node v26.11.1 failed retained v26.12.1 pin. Pinned sandbox run showed fs.watch EMFILE and ps EPERM; scoped unsandboxed execution was granted. Installed local v26.12.1, used its absolute bin directory on command PATH without changing defaults; shim rebuild was denied. Setup failures are separate from product failures.
- No mutation control ran. No final Rust compilation/fmt/clippy/nextest/release check, TypeScript typecheck/lint/format/final suite, runtime proof or CI passed for this diff. git diff --check passed before packaging only.
- CI18 remains open. Read carrier source/tests; hypothesis: child-exit selection cancels buffered outbound forwarding. No forced race, reproduction or causal fix. Do not report the hypothesis as root cause.

## Approved decisions, unimplemented

Report only to claude-local/9f6bdec4-853f-4d3a-8ed0-0d7b4b128af3 on service 0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89.

- 01a18721-caeb-7213-ae7e-193a9562feca: packet's exact 14-crate W2 scope plus scripts/verification; W2 version 0.1.118; merge main before final delivery push. Self-proof then Lead-commissioned Sol review. No board posts; tmp/worker trace.
- 01a18727-fc72-7b31-92ac-0e23889ee717: crate-private auth refresh transport seam, blocking reqwest timeout/current certainty categories unchanged. Fake complete-request receipt precedes injected timeout outcome; preserve outcome/token/state assertions. Uncontrollable dependency timeout mapping branch may be review-only X, explicitly reported. Prove connect-refused mapping without waiting.
- 01a1872a-30ee-7653-834a-3945122546cf: keep real central OS-child restart. Test-support-only clock-taking TokioServer path reuses serve_platform; T0 first child, T1 past expiry after restart, no post-restart SetTime. Shipping constructors keep SystemClock. Prove feature exclusion from shipping CLI/agent-collaboration and startup reconstruction mutation sensitivity. Shipping API/behavior changes remain forbidden.
- 01a18724-01de-7b91-855d-423d8a3285ab: W1 changes local access-expiry helper to existing platform.clock. Lead forwards entry point for access_carryover.rs and session_scope_workerd.rs. Real Cloudflare edge expiry is staging-only X, excluded from CI.

## Remaining

Most A/B conversions remain: quota reset/other floor tests; CLI/test-support readiness/child exit; auth in-flight expiry/retry; central deadlines/retry/expiry/restart; native-adapter polling; W1 helper callers. Inspect indirect timers too. Keep C bounds and assertions; no retries/timeout increases.

Then CI18 causal fix, three wrong-product mutation checks, required gates, matched-workload timing comparison, version 0.1.118 if still unused, main merge, one PR/CI watch. Lead owns review/integration.

## Cleanup and safety

Known commands settled; post-stop inspection found no cargo-nextest, vitest or executable from this worktree target. Production Router/Host and real Codex state untouched. target/debug (31 GiB) held because no further cargo commands are permitted; no cargo clean. No proxy, W1, upstream fork/pin, Python, file over 1,000 lines, allow attribute, stash, amend or no-verify change.
