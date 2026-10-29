# W1 no-wall-clock conversion

## Commission and current constraints

- Base: `881f778c77b4d5532ef1be32f0bd2f44daaf73f5`, branch `test/no-wall-clock-host`.
- Owner packet: inventory before conversions, convert A/B, retain causal timeout bounds, three sensitivity controls, package gates, release check, one PR and CI wait. No proxy, production process, real Codex home, upstream pins, Python, suppression, history rewriting, or parallel Cargo commands.
- Lead clarification `01a18721-cad9-7d81-8587-6e18cc86082f`: exact W1 scope in `scope.txt`; version 0.1.117; no paired verifier now, Lead commissions independent reviewer at final head; `tmp/worker/` is the authorized trace/evidence home; no board posts and reports only to Lead.
- Work-home gap: installed CLI uses the current workspace/board/project domain rather than the skill's old board project/thread surface; no board MCP advertised. Lead explicitly selected this worktree's trace instead.
- Free disk before build: 224 GiB; initial target absent. Build stop threshold remains 25 GiB.

## Decisions and source findings

- Inventory starts with a broad source scan (including production timer owners and timestamp-only context); ambiguous entries remain review candidates rather than claimed finished classifications.
- JWT expiry: Lead `01a18724-01ca-75e1-8dde-1b5b55da1f75` permits class X only for real staging JWT expiry. Local callers must drive the existing edge application clock. No new remote auth-clock control; W1 owns helper, W2 owns both local callers.
- Wake clock: current SubscriptionClock lives above collaboration-runtime. Lead `01a18726-204b-7db1-b302-7ff861bd2a54` approves existing runtime_primitives::Clock in wake worker/sender, adapted from the same SubscriptionClock at composition. Preserve the five-minute minimum, real workers/store/native route, and Tokio tick.
- CI16 already pauses Tokio. Its test observes DueRoots before delivery selection, then waits on an owner barrier; source tracing is still in progress. No root cause asserted.

## Validation baseline

- First nextest command failed before compilation because the selected packages did not include codex-router-cli, so Cargo rejected its qualified keychain-test-support feature. No test result credited.
- Corrected baseline: selected 11 likely-touched packages with all features, `--profile ci`, same source head, output `before.log`. One Cargo command at a time. This is a workload baseline, not a flake-elimination claim.

## Open work

- Refine the full A/B/C inventory and ownership of live/manual journeys.
- Convert assigned cases, including CI11 and CI16, without weakening assertions or changing bounds.
- At least three product mutation controls, restored greens, quality/nextest/release/module-cycle gates, version, PR, CI, and cleanup remain unperformed.
- No product changes or runtime proof claimed at this checkpoint.

## 2026-10-29 owner stop and WIP handoff

Lead push `01a18736-0175-7650-a32e-ccbb6fd226a0` supersedes local validation and final PR work: stop local Cargo/nextest/pnpm tests, commit WIP unsigned, push, hand back once, continue validation on Sunclaw. No further Cargo/test commands are authorized on this Mac.

- Interrupted our nextest session 46362 with SIGINT; exit 130. A filtered process inspection confirmed no remaining process or signing child using this worktree's target.
- Baseline output ends during binary listing, before any nextest test results. Compilation of the old source finished in 5m44s; whole interrupted command was 1087.41 s including compilation and signing/listing. This is NOT a before-test duration or a passing test run.
- Partial source changes: exact AGENTS rule; wake worker/sender clock injection and shared existing MailClock adapter moved as SubscriptionRuntimeClock; test-only wake pass observation and paused recurring test with committed completion fence; remote replay's injected-clock advancement; MCP activity observation and removal of fixed registry waits. None compiled or ran after editing.
- Still open: inventory refinement, most A/B conversions, CI16 diagnosis, local Access-expiry helper/caller integration, workerd compile-only clock design/build/deployment guard, three sensitivity runs, all quality/test/release/module-cycle gates, version bump, PR and CI.
- Lead `01a18731-e221-7f52-bbf5-dd8f8184f953` allows a compile-time local-conformance-only workerd clock only with demonstrated artifact separation and a deployed-build rejection check. No such feature, control, helper-signature change, or W2 call-site edit has been implemented.
- Shared helper signature and mechanical W2 call-site/integration ownership were sent to Lead in `01a18735-44c3-7a80-b30d-86fc594da274`; no answer at this checkpoint.
- Cleanup candidate: the exact, non-symlink worktree target (95 GiB), created by this task; no active consumers; raw baseline evidence kept outside target. Removed with the cleanup skill's generated-output allowance, honoring the newer prohibition on Cargo commands.

The task is in progress, not ready for review or merge. Handoff is `implementation-handoff.md`; continuation prompt is `copy-paste-prompt.md`.
