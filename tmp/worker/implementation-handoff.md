# W1 checkpoint — CI comparison green, local proof held

2026-10-30. In progress; no readiness or acceptance claim.
Worktree: /Users/shravansunder/code/projects/codex-router.test-no-wall-clock-host
Branch: test/no-wall-clock-host. Pushed repair head:819789a4572ff6f874eb30e28136d295a400a66f.
Draft PR194: https://github.com/agent-collaboration/agent-collaboration/pull/194

## Authority and delivery

Direct W1 commission and recorded Lead decisions govern. W1 is an unpaired Worker; Lead owns acceptance and independent review and commissions the final-head PR-body Researcher. Do not self-author the PR body or commission helpers.
Current version is0.1.118. Latest order: PR193 merged/released0.1.117, Claude-fix PR1950.1.118, W1 PR1940.1.119. Main cb01dffd3e60b29e814495fd676844690b07e313 is included in the integration checkpoint per Lead01a18b4b. The819789a4 proof precedes that integration; combined-head CI remains unverified. Do not bump119 until Lead says PR194 is next to merge.
No merge, tag, production replacement or live-provider acceptance authority.

## Current build authority

Lead01a18b33 released focused local cancellation red/green, Clippy and archive measurement under30GiBfloor. Lead01a18b43 then narrowed it: finish only the current cancellation red/green, then hold further Cargo for the priority Claude remote-safety fix. CI comparison continues.
The exact-owned-process disk guard stopped cancellation compilation at05:26:02Z,32146333696bytes free, below30GiB. Exit130; zero tests ran. This is neither red nor green evidence.
Temporary pre-repair explicit-resume control was restored exactly to committed source SHA6233eda0582fc0e489ff64566202b0c3bdd759ccde3c2c21e913b15f43b1a069. Current tracked source is clean apart from the W1 trace.
Fresh process/lsof checks found no own Cargo/rustc consumers. Another worktree owns the remaining Cargo process; it was not touched.
No new local Cargo until Lead explicitly resumes. Clippy and archive measurement remain pending. Prepared archive probe has made zero tracked edits.
Retained release output is3.0GiB by fresh du; no release cleanup grant or cleanup performed.

## Implemented repairs at819789a4

1. ControlledHostWakeClock uses a Drop guard to resume Tokio time if advance is cancelled at its real yielding point. Permanent regression polls the actual advance once, drops it, then checks pause works again.
2. Startup diagnostics retain the real PID observer and existing1s blocking guard, hold actual startup across the unchanged250ms policy deadline, and resume before native child cleanup. Original exit23, SIGTERM, live sleep30 and reap/trace assertions remain.
3. Journal long-poll enters and holds the actual empty read across its unchanged10ms policy deadline, resumes, then applies the unchanged1s native completion bound. Record/cursor/caught-up/empty/close assertions remain.
Direct pinned rustfmt and git diff --check passed. New local compilation, Clippy and red/green outcomes are NOT established by the interrupted build. Current Linux CI separately proves all three repaired cases and the permanent cancellation regression; it does not supply the requested local fault-control red.

## Required CI comparison — authoritative method

Lead01a18b08 supersedes all prior local full-baseline/full-after protocols. Do not run the17-package local suite or recreate a local baseline.
Before: main CI40826492254 at881f778c77b4d5532ef1be32f0bd2f44daaf73f5.
After: PR194 CI41004749636 at819789a4572ff6f874eb30e28136d295a400a66f, completed SUCCESS.
Both use the identical workspace command from ci.yml, including CLI keychain-test-support and the same ten E2E/timer binary exclusions. This command does not use all-features or locked; do not conflate it with the retired local script.
Main full job log is byte-exact and retained. Select only its actual workspace phase for comparison; the separate80-test and1-test phases remain in the full raw source.
Baseline index main-881f-ci-outcomes.index.json:5824present=5728pass+0fail+96skip, suite2414.835s.
Repaired-head CI is overall SUCCESS: Worker12m10s, Static28m13s, local E2E25m47s and workspace1h1m53s green. Workspace5742pass,0fail,96skip(5838present),suite2008.539s. All three repairs and the new cancellation regression pass.
Blocking watcher26799 completed exit0 at interval240; its full69,318-byte output is sealed byte-exact in pr194-repaired-ci-watch.log-receipt.json. No CI watcher remains active.
Full metadata and workspace job123896989639 logs are saved and sealed. pr194-repaired-ci-outcomes.index.json covers every final identity. ci-baseline-vs-repaired.index.json proves5841union,5821retained allsame-status(5725pass,96skip),17added,3removed,0statuschanges. All20name changes are source-explained in ci-repaired-name-explanations.jsonl; three removals pair with relocated additions.
All helper inputs/outputs are SHA-verified, raw logs byte-preserved, parts capped800lines. Never silently drop unknown outcomes or phases.
The pre-repair rehearsal remains historical. Final before/after commands match exactly; both use Rust1.98.1,nextest0.9.137,ubuntu-26.04 image20261025.179.1. Observed suite2414.835s→2008.539s(-406.296s,16.8%); no cold-compile or causal performance attribution. Separate local E2E phases28/0/0skip,54/0/1skip,6/0/0skip and module-cycle1/0/22skip are preserved independently.

## Archive observation — pending, no fix authorized

Lead01a18a70 authorizes one instrumented feature-head run of request_effects_cli_resumes_a_signed_page_without_normalizing_the_token in agent-collaboration::board_request_effects_cli. Baseline instrumentation only if cheap; the new CI method does not authorize rebuilding881f locally.
No timeout, acceptance oracle, board/effect count, retry or product change. Keep20s SDK bound,257created boards,259effects,256/3pages and original token/digest/assertions.
archive-probe-builder.mjs is prepared and syntax checked, not executed. It validates every source anchor before writing, records original/mutant hashes without source copies, and temporarily logs actual request/frame correlation, CLI/SDK/transport/session/transaction/finalization times. Remove all instrumentation and verify exact HEAD hashes before any commit.
Record exact CallFailure before mapping to PendingUnreachable and actual correlated Decided, including late answers if observed before original teardown. No extra waits.
SDK request start to actual send includes the queue, frame allocation and serialization; no pinned SDK/cache edits. Transaction entry gap includes the native connection mutex and BEGIN IMMEDIATE; do not attribute the whole gap to a specific lock without evidence.
O(n²) repeated workspace-board reads during finalization is a source hypothesis only. Measure before proposing. Total test46.9s is not a47s deadline and does not establish a20s request timeout cause.
Main CI has PASS46.915s at881f and PASS29.236s at03b. No preexisting failure has been established. Local interrupted baseline produced zero outcomes.
Lead decides performance/fixture repair after the breakdown. PR194 blocks on this only if evidence ties it to W1; do not decide acceptance yourself.

## Preserved scope and proof

Frozen scope33folders. W2 owns serve_then_watch and its three reserved callers, plus the three sqlite-pressure TypeScript files. They remain unchanged. W1 owns only the approved Access/revocation seams and mechanical cross-lane edits.
Previous focused receipts remain in canonical proof-receipts.json and logs: latest59/0, owner-clock15/0, process25/0, three product controls restored/pass, real workerd8/0, default/release API exclusions, production Worker marker rejection, release builds and native/wasm Clippy. These precede later repairs/version changes; report that distinction.
Source inventory453retained-bound rows,236timestamp origins,177elapsed/negative contexts and25field consumers. Cooperative polling is separate deferred debt, neither converted nor compliant.
Preserve all explicit Lead exceptions for ignored/live Codex, accepted manual external-state reads, the one pinned protected-input case, compiled CLI deadline, native Busy and compiled Host expiry. Staging/live-provider/cross-machine proof remains unrun.

## Next work

CI comparison and inventory reconciliation are complete at the declared repair head. Keep proof for819789a4 separate from future main/version integration.
After explicit local resume, finish cancellation red/green, required scoped Clippy and one archive measurement; stop at30GiBfloor. No local full suite.
Canonical handoff, copy-paste prompt, inventory and receipts reflect the current evidence. Refresh main/PR state before final delivery. Version119 waits for Lead next-to-merge grant. Preserve required final integrated-head release check and CI; older release/exclusion receipts remain historical evidence.
Commit scoped docs/source as needed, push this branch, send exact final head to Lead for Researcher body. Keep PR draft until governed review/readiness; no merge.
Complete applicable current-head checks/comments/reviews/threads/body/mergeability and quiet poll. Then consumer-check and clean only approved own artifacts, report physical free space for other lanes.

## Tools and constraints

Pinned Rust1.98.1, Node26.11.1, pnpm12.14.0, nextest0.9.137, worker-build0.8.7, cargo-modules0.28.0. Use task-local pinned PATH and DYLD fallback; do not fix globals.
One Cargo OR pnpm at a time. No Python, source copies, cache edits, forks/pin changes, lint suppression, timeout increase, assertion deletion, statistical retries, stash/amend/rebase/force, foreign cleanup or production actions.
Current authoritative live state is live-run-checkpoint.json. Canonical docs now reflect repaired-head CI and the remaining holds; this is an in-progress checkpoint, not a final handback.

Latest hold01a18b58: Lead reports Time Machine snapshots retain deleted build files and has asked owner about thinning. No local build until explicit Lead resume; no snapshot commands authorized. This diagnosis is Lead-sourced, not independently verified by W1.
