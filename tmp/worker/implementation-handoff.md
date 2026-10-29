# W1 implementation checkpoint

Date: 2026-10-29. Stage: in-progress; not review-ready or merge-ready.
Repo: /Users/shravansunder/code/projects/codex-router.test-no-wall-clock-host
Branch: test/no-wall-clock-host
Base: 881f778c77b4d5532ef1be32f0bd2f44daaf73f5 (0.1.116).
Head: checkpoint commit containing this file; exact SHA is sent to Lead/W2.
Plan identity: none. Authority is the direct W1 commission in packet.md and the Lead decisions recorded in trace.md. Delivery is one eventual PR to main, CI watched at interval240, no merge authority. W1 is explicitly unpaired; Lead commissions independent review after completion. No independent review has occurred.

This checkpoint preserves partial work and gives W2 its assigned blocking-worker lock observation entry. W1 continues the original assignment. Inventory classification, remaining conversions and required gates are incomplete.

## W2 integration entry

In codex-router-secret-store::account_credential_lock:

```rust
#[cfg(all(feature = "test-support", debug_assertions))]
pub fn acquire_with_contention_observer(
    database_path: &Path,
    account_id: &AccountId,
    contention_observer: std::sync::mpsc::Sender<()>,
) -> Result<AccountCredentialLock, AccountCredentialLockError>
```

Call it on the actual auth blocking worker. It arms that thread's observer, calls the same acquire implementation, reports the real OS WouldBlock result before the existing blocking lock, and clears the observer on return. An uncontended acquisition disconnects the unused sender. Dropped receivers do not alter lock acquisition. Normal acquire's signature is unchanged. The entry and observer implementation are absent without test-support and in release builds, including release builds with test-support accidentally enabled.

W2 must retain its two approved shared Access helper caller changes and its ownership of serve_then_watch and the three reserved callers. W1 owns the approved Core/Cloudflare/Tokio revocation observation change end to end. See lead-lock-observer-seam.md and lead-causal-seams-decision.md.

## Implementation and proof

Raw command details and failures are in trace.md; a scoped green does not complete the assignment.

| Scope | Current evidence |
| --- | --- |
| Repeating wake CI11 | Existing Clock injected through the same SubscriptionClock adapter; real store/native worker retained, 300-second minimum unchanged. nextest 1/0, suite2.017s, command5.66s, exit0. |
| Drop lifecycle CI16 | Held actual presence probe exposed the missing publisher completion fence; intended red, then corrected real lifecycle test green. Expanded registry/CI16 batch6/0, suite3.087s, exit0. |
| Registry and remote wakes | Dropped-handler notification regression red then green; remote clock cases5/0, suite2.958s, exit0. |
| ACP cancel wire order | All three cases pass with held reply and actor FIFO fence. One deliberate drain-bypass fault failed all three; exact restoration passed3/0. Two additional distinct mutation controls remain. |
| Native Access expiry | Real HTTP/RSA before/after caller-driven signed-expiry+5s. Full Access suite5/0 in runtime batch. Workerd caller compiled; actual Worker expiry remains unverified. |
| Revocation typed waits | Feature-only subscribe-before-read, actual owner checkpoints, drop cleanup, independent client predicates. Native3/0 and progress-observer3/0 in runtime batch. Worker path and production exclusion proof still open. |
| Host causal batch | Current CI9, PID/reap, dispatch/cancel/shutdown, auth-file watcher, retention and wake composition:11/0, suite12.530s, command405.29s, exit0. CI9 source unchanged; historical cause unclassified. |
| Cross-lane lock entry | New worker contention/observer cleanup plus existing lock/FIFO cases:6/0,88skipped, suite0.211s, command8.44s, exit0; lock-worker-observer-green.log, run9db4c0ca-f157-44b6-81f4-e1e6f2f588f2. |
| Scripted pinned TUI | Official Codex0.157.1 asset SHA verified in task scratch. Four UI cases and held partial-frame regression pass. Trust conversion still fails; original committed trust case passes. Early Enter sends no config/batchWrite; original delayed case does. Temporary RPC instrumentation and original-case swaps were restored exactly. Investigate upstream startup/protected-input state before another fix. |
| Production Worker | Normal worker-release prebuild and production marker guard passed before latest revocation addition. Local clock enabled artifact, actual workerd scenarios, deliberate production guard rejection, release feature rejection and final normal rebuild remain. |
| Quality | cargo fmt --all -- --check passes. Runtime test-source check passed. Full touched-package Clippy, nextest, release-bin check, module-cycle ratchet, comparable before/after totals, PR and CI remain open. |

## Source prepared but not yet verified

- CLI interrupt subscription is registered before dispatch; fixture sleep removed.
- MCP request-admission observer is available through test-support; overload fixture waits on actual admission.
- BoardHost serializes its existing control pair; fixture retries use actual replicate/drain controls. CI6 public E2E and pending registration behavior need execution.
- Host idle-health assertion advances paused Tokio, then fences its loop with an explicit status observation, accounting for that observation's one probe. New version unrun.
- Tailnet own-command waits use one joined waitid(EXITED|NOWAIT) thread, unchanged bound, then owning kill/reap on deadline. Graded Host wait and accepted external observations remain unchanged. Stdout readiness and real signal wake replace its fixture polling. Tests pending.

## Remaining work and risks

1. Resolve the pinned TUI trust fixture from real source/protocol evidence. A rendered trust frame alone is not a proved remote-input readiness checkpoint. Upstream startup protected-input/query handling is the current research lead. No upstream fork/pin change or assertion removal.
2. Execute CI6 variants. CI10's requested-cancellation API/test was removed on current main; preserve that retirement rather than resurrecting it.
3. Finish every inventory row's actual A/B/C/context classification and remaining conversions. The inventory remains an incomplete checklist. Preserve the exact Lead exceptions for native live schedule/timeout and accepted external process-state observations.
4. Finish local Worker proof and at least two more distinct fault controls, then full scoped quality/runtime gates and a valid comparable Sunclaw baseline. Existing interrupted Sunbook baseline has no test totals.
5. Merge current origin/main before final delivery; use assigned0.1.117 only if still next unused, under Lead version ordering. Commit unsigned and push only this branch. One PR, watch CI, then final Lead handback/review.

Security: no production Router/Host, real Codex state, global credentials/config, proxy source or upstream pins changed. Local Access control is compile-time gated and release-rejected; ordinary deployed builds omit it. Production artifacts still need final negative/positive exclusion checks. New lock observation has feature+debug gates and no shipping API. Scripted UI fixtures use private temporary Codex homes. Do not infer staging/live-provider proof from these tests.

Builds use explicit pinned tool PATH in trace.md; the global Cargo launcher is broken and must not be repaired here. One Cargo/pnpm command per worktree. Check free disk before heavy/workspace runs; stop below60GiB. This worktree's dev artifacts were cleaned after owned consumers stopped (8001files/82.7GiB removed); subsequent proof rebuilt only needed artifacts. Preserve other worktrees and processes.

Next route: continue the explicitly authorized W1 execution through implement-plan using the direct packet and Lead grants. This checkpoint is not an admission request for implementation-review.
