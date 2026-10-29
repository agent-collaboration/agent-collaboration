# W2 implementation handoff

Date: 2026-10-29. Stage: in-progress; active on Sunclaw, not review-ready.
Repo/worktree: /Users/shravansunder/code/projects/codex-router.test-no-wall-clock-cli
Branch: test/no-wall-clock-cli
Head: 257c6a7d63b32a250ff995fa99c2b1a80a5210f4 plus the current client-admission diff and this trace refresh.
Base: original commission/inventory881f778c77b4d5532ef1be32f0bd2f44daaf73f5; Sunclaw worker created from b893117cc135714d1a9e274cb86f74d79d58746f. Exact W1 checkpoint6dbc117e856c015c46553a45690c81a3969f7cf0 imported by merge7b2088d7 under explicit Lead authority.
Plan identity: none. Governing basis: direct W2 commission, packet.md, user AGENTS.md and pinned Lead decisions in trace.md. No canonical plan was invented. Delivery context: finish all scoped A/B conversions, retain C and assertions, self-proof, one eventual PR/CI; Lead commissions independent review. No independent review/acceptance has occurred.

The original machine-stop handoff is preserved at imported-w2/pre-sunclaw/. Its prohibition was for the predecessor Mac/transport stop; the active Sunclaw execution grant supersedes that historical location-specific stop.

## Objective and authority

Tests never sleep/poll to let work happen (A) or require real elapsed duration to pass (B); use real causal signals or the owning clock. C is a causal-wait failure bound and stays. Do not raise timeouts, add retries, remove outcomes, invent upstream pins/forks or suppress lints. Source files stay below1,000lines. No Python.

Exact14-crate W2 scope and scripts/verification are in packet.md. Proxy is excluded. STG-H1–H4 were removed by Lead01a18894-77eb-79a1-98e0-341605f28ac9: their unpushed pressure harness is deferred. Do not invent it or run staging. W1 owns all remaining other work plus the Access helper/mechanical callers and RevocationControl typed-wait/adapters. Preserve its imported changes; do not edit its AccountCredentialLock module.

Relevant grants:01a18727-fc72-7b31-92ac-0e23889ee717 private blocking OAuth exchange seam/review-only dependency-timeout X;01a1872a-30ee-7653-834a-3945122546cf real OS-child central restart with T0/T1 supplied clock, same serve_platform, no post-restart SetTime;01a188a1-780a-77d2-b215-b1e1b6cfbed0 explicit fake-API completion cutover and existing native fixture control socket;01a188f4-7af1-7861-b62f-e0f0cb9944b8 feature-only quota cycle waiter;01a1890c-a6fd-7041-b4c5-11f5d1bbe4b9 W1 lock observer and W2 activation/CLI wiring. Full decisions/reasons and remaining conditions are in trace.md.

Report only to 🦁 Lead at {"endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},"sessionId":"9f6bdec4-853f-4d3a-8ed0-0d7b4b128af3"}, once per verified batch via agent-collaboration message send --text-file. No board posts; tmp/worker is the explicit work-home/no-board trace. Current W2 identity is codex-local/01a1888f-5ae3-7892-9672-266dff36eaa7 on service8b61218b-d5f3-4ae9-a2db-d2fb4075dbc2; verify your own actual session identity before using tools, never impersonate this reference from another session.

## Current state and changed responsibilities

Pushed checkpoints:a96feaac,cbc82148,236606c9,6104fb93,merge7b2088d7,e00bccd8,257c6a7d,71a2cd73,b95ce136. No PR; workspace version still0.1.116. W2 version0.1.118 is assigned but must still be unused; W1 lands first, then merge fresh main before final delivery. No merge/tag/publication/production replacement authority.

- Fake API inventory/helper and three granted agent-collaboration callers: explicit operation/child completion, retained transcript/further-call checks and direct kernel queue inspection. Two permanent observer controls.
- Native carrier: permanent CI18 real backpressure+actual-child-exit regression; supervisor drains an attached producer after exit within unchanged settlement bound. Real backend-loss kill/reap case uses a private lazy deadline.
- Auth: held secret-read/refresh with controlled clock, real-instance/process rotation signals, typed actual persistence retry facts plus virtual250ms, real HTTP device polling under client-armed virtual clock. Blocking Claude exchange private seam preserves request/certainty behavior; complete real POST receipt precedes injected SentThenTimedOut. Real connect-refused mapping covered; actual dependency blocking timeout mapping is review-only X.
- Quota: frame/completion receipts replace input sleeps, test-only cycle permits keep real CLI resolver/SQLite cycles, real HTTP timeout driven after received request. Native adapter uses existing fixture control socket and exact file bytes/acknowledgements. Original Rust PTY polling remains partly open.
- CLI upkeep: existing control owner gives read-only completed-cycle receipts; real shutdown Pending/drain/Stopped events replace quiet windows and generation polling. Login race carries W1's debug/test-support contention observer through actual auth blocking worker, keeps original generations/tokens; public import request equality unchanged in focused account_import.rs. Observer journey is debug-only by the approved release-excluded seam, mandatory CI includes it; no release-unit-test proof claimed.
- CLI transport tests: existing actual listening-address line drives clients; all21 retry helper definitions/uses cut over, stdout preserved. Fragmented/malformed receipt controls. Stop timeout: private HostStopDeadline and cfg(test)-only CLI exit instant retain real socket/kernel/lock path and normal40s budget, removing the real40s success prerequisite.
- Native client negative windows: operation completion then direct buffered/kernel inspection; reset Cancel directly observes its real pending future/Inspecting before authority release.

Batch6: admission saturation now uses the actual running-call receipt;11/11 passed. Imported MCP expect corrected to the same invariant assertion; combined client/MCP clippy and fmt pass, registry lifecycle5/5 passed. Batch8 now verified: private sync owned wait, real pipes/EOF-controlled expiry, actual stdout readiness and reader joins; full test-support50pass/16ignored,6.531s,0b664d93-3c1a-482d-bf4d-0e85bb07e929,clippy/fmt0. No command is running at this writing. Check status/handles later. Central startup observation approved01a1895c and implemented: real T0/pre-expiry/T1 children, actual alarm receipt then direct owning-store assertion before requests. Final7/7green,clippy/fmt0,default-only feature graph. Scope and remaining indirect timers still open.

## Implementation proof and limits

All receipts are under tmp/worker; commands/counts/run IDs and source corrections are recorded in trace.md/inventory.md. These are scoped results at their stated source snapshots, not whole-lane acceptance.

- CI18 pre-fix regression failed with the original ResetWithoutClosingHandshake, exit100,0.379s; after repair carrier/redirect7/7,exit0,0.591s,52e55a16-7be3-4fea-bf1d-b5c19d2ce52c.
- Three wrong-product controls: stale read/refresh clocks emit expired credentials (2failed); retrying floor wrongly resaves (expected1/actual2,1failed). Exact sources restored byte-for-byte. Restored controls+startup4/4,exit0,f392e59a-076d-4855-bb42-18aab01df679. Do not rerun or call these pre-fix TDD.
- Full TUI TS package154/154,exit0,7.15s; typecheck/lint/format exit0. Real native PTY/controller3/3,exit0,4.095s,d401391e-2e70-48e5-bf82-6c54864012cd. This proves apparatus interaction, not real provider/UI acceptance.
- Full auth+quota-service libraries220/220 (80+140),exit0,7.169s,dfac8bac-bf5f-4637-8f57-9acf59f1c0c1. Native client2/2,exit0,4f2505f7-8086-4f2a-9375-e292c493b11c.
- Final lock/login/upkeep integration14/14,exit0,2.492s,f713c946-e80c-4be0-b6ec-c396ce8cb80f; earlier exact upkeep8/8,c73ac5a8-ded6-4040-8c35-e0e4f19f4868. Full selected-package all-target/all-feature clippy-Dwarnings and fmt checks pass at these checkpoints. One obsolete W1 async-test lint expectation removed only, no assertion change; W1 notified.
- Listener/WebSocket20/20,exit0,3.354s,f35dfe96-eedf-4de3-9fef-377d2f1ce3d6. Stop/budget/writer7/7,exit0,0.934s,1363f083-61f2-40c8-8350-a586a6b5f7ec. Final full CLI291passed/1existingignored,exit0,34.197s; same package/cases before stop clock50.939s (separate diagnostic audit50.279s found Stop41.142s; after1.043s including runner overhead). These are a limited matched CLI snapshot, not the original14-package total or statistical benchmark.
- Current dirty admission integration11/11,exit0,2.554s,573fab7b-edb7-4e77-90f2-6ce7794f7db5; real holder entered before overload probe, no sleep/restart. Combined client/MCP clippy and workspacefmt pass; registry5/5 passed,exit0,0.841s,43665f8e-20b0-4ae2-9037-4285658238dc.
- The original14-package baseline was interrupted:471/1858run,447passed/24failed including interrupt settlements,1387unrun;268.765s runtime,1053.73s command. It is not a valid complete baseline/comparison. Need an honest complete comparable total, the same worktree/target at detached base881f778c77b4d5532ef1be32f0bd2f44daaf73f5 after committing current work, per Lead01a1895b; no git archive or second target.
- W1 import is a partial checkpoint, not final W1 acceptance. Its own remaining TUI/live/release gates remain W1's. Five conflicting documents were byte-preserved under imported-w1/6dbc117e and own live paths. No source conflicts; both dev-dependency additions retained.

## Unfinished work / next route

Continue through implement-plan under the explicit direct commission, not a fabricated plan; do not request review yet. After batch6 commit/report, continue direct owned child/process/CLI integration/PTY fixture signal conversions and central clocks/restarts. All remaining A/B and ambiguous grouped inventory rows need individual verdicts, including indirect timers. Current scan has1,132sites/434groups; many non-oracle timestamps/C guards have source review, not every row is closed.

Central approved restart path now implemented for Copy staged parts and Registry Claim notice expiry: preserve actual child death/reopen and startup reconstruction, inject T0 first/T1 past expiry on second startup through existing platform Clock/serve_platform, no post-restart SetTime. Retentions and exact identities preserved; wrong Copy alarm completion without pruning failed1vs0,exact source restored. Final7/7pass,8596beb0-dfa3-4835-86f6-472c272eec16. Remaining real30s recovery/continuation helper rows still open. W1 owns only its RevocationControl adapter parts; coordinate other shared conformance changes through Lead.

Final required gates: fmt; touched-package all-target/all-feature clippy-Dwarnings; nextest--profileci for all scoped touched packages, CLI keychain-test-support; cargo check --locked --release -p codex-router-cli -p agent-collaboration -p board-legacy-upgrade --bins; release/default feature exclusions; honest totals. Then fresh main/W1 integration, unused0.1.118/Cargo.lock, one PR, gh pr checks N --watch --interval240, complete self-proof then Lead's independent Sol review. No CI/delivery/acceptance gate is complete.

Do not redo completed mutations/CI18 diagnosis or broaden live staging. No production Router/Host, real Codex state, proxy edits, credential copies, installed binaries, upstream forks/pins, dropped outcomes, timeout increases, retries, lint allows, stash/amend/force/rebase. Real OS/file/socket interactions stay real; stand-ins never count as integration proof. Recorded source-only, runtime, CI, delivery and acceptance remain separate.

## Operational state

Use the owned worktree explicitly for every command; default cwd is the main checkout. Exact Rust PATH for each invocation: /Users/shravansunder/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin:$PATH (the normal cargo shim points at missing rustup-init). Node26.12.1/pnpm and the retained TUI lock are installed. Do not change global tooling or install unreleased binaries.

One Cargo/pnpm per own worktree; workers use their own targets, no shared slot/lock. Check free disk before heavy runs and stop below60GiB. Last255GiB free,target/debug26GiB; verify anew. Keep progress updates within60s and never end with a live command. Socket/process tests need the actual require_escalated host grant. Renew exact worktree write permission when new user input resets the turn grant; never remove a lock or bypass an access denial.

Signing runner signs nextest executables but a later Cargo invocation can republish root child binaries linker-signed. For direct-child final gates: finish preparing build metadata first, sign exact owned target/debug children with scripts/cargo_debug_signing_runner.sh --sign-only, read back identities, then use nextest prepared metadata without rebuilding between sign/launch. Prior6/6 shared CLI functional result is distinct from later code-identity readback. Normal debug/released identities stay separate; production is never replaced.

Keep failure logs/receipts. External workers-rs template parser diagnostics can appear while selected clippy commands exit0; do not edit that external cache or misreport them as selected lint failures. Do not clean own target until no active consumer needs it; preview cleanup and remove only task output when proof is done. Directory removal/worktree deletion still needs explicit permission.
