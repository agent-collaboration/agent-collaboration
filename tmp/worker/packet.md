# No wall-clock tests — W1: Host / collaboration crates (Sol worker)
Worktree = cwd, branch test/no-wall-clock-host from main (0.1.116).
Scope: crates/codex-router-host, crates/collaboration-* (except collaboration-central-tokio, which W2 owns), crates/agent-collaboration, crates/message-board*, crates/interaction-storage, crates/automation-storage, crates/lifecycle-observation, crates/workspace-architecture-check, crates/tailnet-verification-host, and any other crate not listed in W2's scope EXCEPT codex-router-proxy.
Known items in scope (Lead's bug ledger tmp/plan-workflows/2026-10-24-bug-ledger.md in the codex-router.collab-improvements worktree): CI11 — collaboration-service wakeup_repeating_delivery::recurring_wake_stores_and_dispatches_two_distinct_pushes waits the real 5-minute repeat gap: the Lead already approved injecting the existing SubscriptionClock into WakeTimingWorker with a test clock tied to paused Tokio, real worker/store/native delivery kept, product five-minute minimum unchanged, covering WakeDeliverySender claim/completion too. Also CI6, CI9, CI10, CI16 (subscription_delivery lifecycle_proofs::drop_then_push_leaves_both_messages_unread 300 s hang), CI18 (agent-session-inventory source_native_carrier websocket reset — W2 owns that crate, skip), STG-H1/H2 harness oracles if they live in your crates.
Also add to AGENTS.md (Testing section or nearest) exactly this rule, nothing more: "Tests must not depend on wall-clock time: wait on a causal signal or an injected clock; a timeout may only bound a causal wait."
## Rule (owner, 2026-10-29: "there should be no wall clock tests")
A test never sleeps to let something happen and never needs real elapsed time to pass. Classify every real-time use in your scope:
- **A — sleep/poll as synchronization** (sleep then check; retry-with-sleep loops; "give it a moment") → replace with a wait on the real signal: store state change, watch/notify, channel, acknowledgement, child exit, readiness line, event observer.
- **B — real-duration behaviour** (e.g. a 5-minute repeat gap, expiry, debounce, deadline logic under test) → inject the clock the product already has (or the narrowest clock seam at the owning boundary) and drive it from the test, or use paused Tokio time where the code is Tokio-timer based. Product minimums/semantics unchanged.
- **C — timeout bounding a causal wait** (`timeout(BOUND, wait_for_signal)`) → allowed; keep the bound, it is a failure guard not an oracle.
Never fix anything by raising a timeout, removing an assertion or adding retries.

## Deliverable
1. Inventory first: `tmp/worker/inventory.md` listing every real-time use in your crates (file:line, class A/B/C, plan). Hand nothing back yet; continue.
2. Convert all A and B in your scope; keep C. If a B case needs a NEW product seam (a clock parameter in production code), use an existing clock abstraction if one exists (e.g. SubscriptionClock for wake timing); if none exists and the seam would change a public API or stored data, stop and report.
3. Each converted test must still fail for the wrong product behaviour: spot-check by breaking the product behaviour in scratch for at least three converted tests and showing failure.
4. Gates: fmt; clippy -D warnings on touched packages all targets/features; nextest for touched packages (`--profile ci`; CLI needs `--features codex-router-cli/keychain-test-support`); release check `cargo check --locked --release -p codex-router-cli -p agent-collaboration -p board-legacy-upgrade --bins`; report total test wall time before/after for touched packages.
5. One PR to main from this worktree; version: next unused at merge time (the Lead assigns; main is 0.1.116). Wait for CI with `gh pr checks <n> --watch --interval 240`. Hand back once.

## Rules
Report only to the 🦁 Lead (claude-local 9f6bdec4-853f-4d3a-8ed0-0d7b4b128af3). Never touch the production Router/Host or real ~/.codex. Do NOT touch crates/codex-router-proxy (owner rule). No upstream forks/pins. No Python. No file >1,000 lines. No `allow` (`#[expect]` with reason). No git stash/amend/--no-verify; commit --no-gpg-sign. Check `df -h /System/Volumes/Data` before heavy builds; stop under 25 GiB; clean your target after. One cargo command at a time; don't end your turn while a command runs. Design break → stop and report.

## Priority order (Lead, 2026-10-29) — do these first, before the bulk inventory conversions
1. CI11 (WIP done, unverified): finish + verify on Sunclaw.
2. CI16: subscription_delivery::tests::lifecycle_proofs::drop_then_push_leaves_both_messages_unread hangs to 300 s — find the wait that never completes; causal fix.
3. CI6 / CI9 / CI10: takeover and no-write races (see ledger) — replace elapsed-time waits/snapshots with the real signals (dispatch selected, frontier acknowledged, writer idle).
4. Then the remaining class A/B inventory.
All builds/tests run on Sunclaw, never on Sunbook.
