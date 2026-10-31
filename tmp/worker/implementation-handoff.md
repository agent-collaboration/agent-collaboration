# W1 checkpoint — version 0.1.119, final CI and opt0 probe pending

2026-10-30. This is the source publication checkpoint, not final acceptance.

Worktree: /Users/shravansunder/code/projects/codex-router.test-no-wall-clock-host
Branch: test/no-wall-clock-host. Actual Git HEAD and status govern.
Draft PR: https://github.com/agent-collaboration/agent-collaboration/pull/194

Main `7af23abe970a6d2f7d503be452730fb30d7bb0c9` is integrated at `52686c970416828728bdbf5fb90ebf0ba61741ea`. It includes released PR #193 (0.1.117) and PR #195 (0.1.118). The manifest and lockfile now use 0.1.119: 49 workspace version updates and no external dependency changes.

## Authority and boundaries

The direct commission in packet.md and recorded Lead decisions govern. W1 has no helper commissioning grant. The Lead commissions the PR-body Researcher and independent review after FINAL handback. Keep the PR draft and its body empty until that handoff. No merge, tag, production replacement, or acceptance authority is granted.

Lead decision 01a18c3c lifts the build hold and restores the 60 GiB floor. Its ordered local steps 1–4 are complete. Decision 01a18c67 adds one further measurement: after the 0.1.119 push and CI starts, run the same archive case at default dev opt-level 0, attribute the pre-submit delay, and restore every temporary edit. No fix is authorized. If CI fails only on the archive case again, report it and do not rerun; the Lead assigns it a separate flake lane.

The frozen scope is in scope.txt. W2 retains serve_then_watch, its three reserved callers, and the three sqlite-pressure TypeScript files. W1 has no proxy delta relative to main. Preserve production, configuration, credential, and upstream boundaries, plus the exact exceptions in inventory.md. The existing target is excluded from Time Machine according to the Lead; W1 ran no snapshot command and created no new worktree.

## Completed local proof

| Gate | Result |
| --- | --- |
| Cancellation red | Pre-repair explicit-resume behavior: 0 passed, 1 failed, 1 skipped; exit 100; suite 0.211s. Intended panic: `time is already frozen`. |
| Cancellation green | Exact restored source: 1 passed, 0 failed, 1 skipped; exit 0; suite 0.269s. Source SHA256: `6233eda0582fc0e489ff64566202b0c3bdd759ccde3c2c21e913b15f43b1a069`. |
| Native Clippy | 17 native packages including CLI, all targets and features, `--locked`, `-D warnings`; exit 0, 305s. |
| Worker wasm Clippy | Real wasm32 target, all features, `-D warnings`; exit 0, 56.48s. |
| One opt1 archive measurement | Run `90334ae0-de54-4a6d-a0c8-82d9f3cd027c`: 1 passed, 0 failed, 2 skipped; exit 0; suite 17.061s. All eight tracing files restored exactly. |
| Release check at 0.1.119 | `cargo check --locked --release -p codex-router-cli -p agent-collaboration -p board-legacy-upgrade --bins`; exit 0, 46.14s. |
| Formatting | `cargo fmt --all -- --check` and `git diff --check`; both exit 0. |

The three earlier product fault controls (ACP, wake, Access) are separate from the added fixture cancellation control; all restored and passed. Historical workerd 8/0, default/release API exclusion, normal Worker marker rejection, and forbidden release-clock receipts remain in proof-receipts.json. They establish their declared source/artifact snapshots, not blanket final-head acceptance.

One archive command preparation error selected a feature on an unselected package. It exited 101 before tests and restored all eight files. Corrected package selection then ran exactly one actual opt1 measurement with retries disabled. No temporary Rust test file, source copy, cache edit, lint suppression, timeout increase, or assertion removal was used.

## Completed CI comparison and later failure

The authoritative comparison uses CI, not the retired local full-suite protocol. Do not recreate a detached baseline or a full local 17-package nextest run.

| Workspace phase | Present | Passed | Failed | Skipped | Suite |
| --- | ---: | ---: | ---: | ---: | ---: |
| Main run 40826492254 at 881f | 5,824 | 5,728 | 0 | 96 | 2414.835s |
| Repair run 41004749636 at 819789a4 | 5,838 | 5,742 | 0 | 96 | 2008.539s |

The complete union contains 5,841 identities: 5,821 retained with identical status, 17 added, 3 removed, and no status changes. All 20 additions/removals have source explanations; the three removals pair with preserved module relocations. Both commands and environments match: Rust 1.98.1, Nextest 0.9.137, Ubuntu 26.04 image 20261025.179.1. The observed suite reduction is 406.296s (16.8%); this is not a cold-compile benchmark or causal speed attribution.

Separate repair-run E2E phases passed 28/28, 54/54 (one skipped), and 6/6. The module-cycle gate passed 1/1 (22 skipped), the Rust size gate checked 3,718 files, and all active jobs passed. Raw logs and complete per-test outcomes are hash checked and split into parts of at most 800 lines.

After merging main #193, run 41011565013 at 05b6944c failed only on archive: 5,751 passed, 1 failed, 96 skipped; suite 2228.519s. The archive assertion received `pendingUnreachable` instead of `accepted` after 49.317s total. Its ten added main #193 cases are source-explained; there were no removals and only the archive pass-to-fail transition. Worker, Static, and E2E jobs passed. This failure is retained and was not rerun.

Final 0.1.119 CI is pending at this publication checkpoint. Its exact run, head, outcomes, and complete name reconciliation belong in live-run-checkpoint.json and the FINAL handback. Keep the older repair comparison separate from the combined main #195 / version 0.1.119 result.

## Archive opt1 measurement and limits

The result, exact request/frame correlation, and raw hash are in archive-measurement-result.json, archive-request-stages.jsonl, and the archive-measured-focused log receipt.

CLI archive time was 11.373126s, including 10.053768s before FleetClient.submit. That startup/IPC/API interval remains unattributed. Admission mutex: 12µs; outbox: 571µs; SDK queue/frame preparation: 60µs; send to central admission: 501µs; session admission: 19µs; transaction entry including native mutex/BEGIN: 25µs.

SDK round trip was 1.282518s; central transaction 1.274990s; archive completion 1.194583s; 258 board snapshots 1.176160s. Decision application took 60.313ms, cache refresh 25.157ms, and final synchronization 4µs. The exact matching Decided frame arrived with no SDK error. No AnswerLost or late-answer case was reproduced. Measured wire parsing added 350µs; other logging overhead was not isolated.

These are native Mac opt1 timings, not a reproduction of Linux opt0 CI. Fourteen committed archive-path files are byte-identical to 881f; that source provenance does not completely exclude surrounding changes as a causal factor. No performance fix or test redesign has been applied.

## Remaining execution

1. Publish the 0.1.119 checkpoint and start one blocking CI watcher with interval 240.
2. While CI runs, perform the one newly authorized opt0 measurement. Attribute the pre-submit interval and restore all files to the frozen published head. No smaller data set, new wait, alternative oracle, or acceptance change.
3. Reconcile every final test identity/status, including main #193/#195 additions. If only archive fails, report the Lead-granted separate-lane condition and do not rerun. Other failures require bounded diagnosis.
4. Send FINAL handback only after CI terminates and the opt0 measurement completes. Include exact source head, version, commands, counts, exits, receipts, exceptions, and remaining Lead-owned review/acceptance. Do not commission the Researcher yourself.
5. Preserve receipts, check consumers, and clean only approved own artifacts. Never clean foreign output or stop production.

This committed checkpoint intentionally predates the opt0 probe and final CI. Use live-run-checkpoint.json and FINAL handback for those later results. No CI watcher or local command from an earlier session should be duplicated.

## Tools and constraints

Use pinned Rust 1.98.1, Node 26.11.1, pnpm 12.14.0, Nextest 0.9.137, worker-build 0.8.7, and cargo-modules 0.28.0. run-focused-cargo.sh supplies task PATH/DYLD and opt1 settings; the opt0 probe must explicitly override dev/test optimization to 0. Keep debug assertions and overflow checks enabled.

One Cargo or pnpm command at a time; 60 GiB floor. No Python, source copies, cache/pin changes, forks, lint allows, statistical retries, timeout increase, assertion deletion, stash/amend/rebase/force, merge/tag, foreign cleanup, production action, or system snapshot changes.

Ignored/manual disposable Host matrix, staging Access, live-provider, and cross-machine proof remain explicitly unrun or outside obtained proof. Cooperative polling remains separate deferred debt, neither converted nor compliant. Exact accepted exceptions remain in inventory.md.
