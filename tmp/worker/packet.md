# No wall-clock tests — W2: CLI / quota / client / sessions / central (Sol worker)
Worktree = cwd, branch test/no-wall-clock-cli from main (0.1.116).
Scope: crates/codex-router-cli, crates/codex-router-quota*, crates/codex-router-auth, crates/codex-router-tui, crates/collaboration-client, crates/agent-session-inventory, crates/agent-session-picker, crates/collaboration-central-tokio, crates/codex-native-integration, crates/codex-router-test-support, crates/node-test-fixture, scripts/verification (TypeScript tests too). W1 owns everything else; codex-router-proxy is out of scope for both.
Known item: CI18 — agent-session-inventory sessions::source_native_carrier::tests::source_carrier_forwards_native_child_messages_once_and_cleans_up_after_child_exit panicked at source_native_carrier_tests.rs:188 with Protocol(ResetWithoutClosingHandshake) (main CI run 40826492254): find the causal fix.
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

## Scope addition (Lead, 2026-10-29): the staging/verification harness `verification/` (top-level, TypeScript)
STG-H1–H4 from the Lead's bug ledger are wall-clock ORACLES in this harness and are now W2's:
- STG-H1: hosted direct-mail oracle stops at a fixed 30 s (PeerRecipient.expect deadline) although delivery happened at ~32.8 s → wait for the recipient-observed delivery event; deadline only bounds that wait and its failure must say what was not observed.
- STG-H2: takeover readiness oracle ends at 60 s without deciding late activation vs a stall → decide on the activation/readiness signal itself; report which state was reached.
- STG-H3: archive deadline revokes clients while their Copy backlog is still advancing → wait for backlog drained (archiveProgress reaching the feed position), not a fixed deadline, before revocation/cleanup.
- STG-H4: fresh-client cleanup treats Welcome as inventory discovered and terminates on an empty cache → wait for the inventory-populated signal, not Welcome.
Same rule: no oracle decided by elapsed real time; timeouts only bound causal waits. These run against live staging, so their real external latency stays — but the DECISION must come from the observed state. Proof: unit tests of each oracle against recorded event streams (fast, no network), plus one staging run on Sunclaw if staging access is available there.
