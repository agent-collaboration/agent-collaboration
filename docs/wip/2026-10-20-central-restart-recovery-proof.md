# Native central restart recovery

Source base: `cb930a013d338d7ab2efa702f7dcf5a9407838d0`, branch
`feat/central-restart-recovery`. The library pin is `68394e06`.
The adopted outcomes are one-core Program Design §8 / OC-16 and decisions
497, 551, 555, 561, 577 and 599 in the delivery decision log.

The permanent `process_restart` integration target starts its own executable
as a child running the existing `TokioServer`. Every restart launches a new
OS process on the same isolated data directory. The first server is killed
with SIGKILL only after an existing conformance barrier confirms the crash
point; the parent reaps it before starting the next server. Normal startup,
journal, timers, stores, HTTP and WebSocket paths remain real. No recovery
mechanism, dependency or production code is added.

The child entry is explicitly ignored during normal test discovery; each of
the three parent tests invokes it with `--ignored --exact` and supplies its
own root and fixture JWKS URL. The ignored entry is apparatus, not a fourth
recovery scenario. The held drives retain the existing 30-second journal lease;
the new Unix-only tests wait at most 40 seconds for their outcome. They change neither
existing timeouts nor the clock nor journal rows.

| Scenario | Crash point and real path | Deciding observations |
|---|---|---|
| Fleet notice (497) | Normal enrolled writer commits WorkspaceCreate; Fleet's `HoldNextDispatch` stops the journal's call to the reader's Connection object. | Before death: one pending durable step and zero machine notices. After restart: the reader receives position 1 through Fleet position 2 **before any Fleet request or manual alarm**; mutation replay retains its decision; durable readback finds one task, workspace and notice, with the original task completed. |
| Retirement (577) | Normal owner HTTP retirement passes the real Fleet archive gate and commits its Retiring seal; `HoldNextDispatch` stops Registry's `retire_copy` call. | Before death: the same operation holds four durable steps and Copy still has its seeded row. After restart: Fleet's owner directory converges to Retired **before replaying retirement**; the same operation completes, one retirement task remains, and the project table has zero rows. Its stored result and HTTP replay agree; a third process returns the identical answer without changing the terminal journal record. |
| Registration reply loss (551/561) | Normal Fleet creation and Register on an admitted writer; `DropNextAnswer` suppresses the exact outgoing frame after commit. | The barrier identifies the dropped frame; the journal contains one completed Registration with a real epoch-1 grant. After restart, the original enrolled writer reconnects and repeats its original request twice, receiving that exact stored result. The operation ID, result bytes and one-operation count remain unchanged. |

Retirement setup reuses the existing native retirement fixtures: canonical
archived Fleet graph, public library ImportTask and a real populated seeded
Copy. That arranger is setup only; it does not prove HTTP import orchestration.
The retirement request, archive gate, wipe, publication and restarted recovery
use the actual native server. There is no import or retirement stand-in in the
action being tested.

## Validation

Evidence is text under `tmp/central-restart-recovery/`. Commands run from this
worktree with its own normal `target/`.

- Initial fixture run: one passed, two failed, one ignored, exit 101. One failure
  used a Worker-only bootstrap route (HTTP 401); the other wrongly required
  terminal journal step retention. The library's `SqlJournal::finish` deletes
  the steps when it saves the terminal result. These are fixture/oracle errors,
  not observations of a pre-existing recovery defect.
- Corrected baseline: three passed, zero failed, one ignored, exit 0, 30.29 s.
  `2026-10-central-restart-baseline-2.log` retains the process IDs and outcomes.
- Final narrowed baseline: three passed, zero failed, one ignored, exit 0,
  30.30 s (`baseline-final.log`).
- One valid combined sensitivity build: zero passed, three failed, one ignored,
  exit 101, 41.11 s (`variant.log`, `restart-variant.diff`). Fleet receives no
  owed notice and retirement never reaches the directory. Registration commits,
  loses its answer, restarts, then its replay closes instead of returning the
  stored result. The source files were restored byte for byte afterward.
- Sensitivity preparation is retained separately: `variant-setup.log` failed
  compilation, while `variant-preparation.log` caught both startup omissions
  but broke first-time registration. Neither is counted as registration replay
  sensitivity.
- Full package with `--no-fail-fast`: 254 passed, zero failed, one ignored,
  exit 0, across 63 test/example targets (57 nonempty). The child entry is
  counted only as ignored; child runner output is drained rather than counted
  a second time (`full-package.log`, `full-package-receipt.json`).
- Package all-target strict Clippy (`-D warnings`): exit 0
  (`clippy-package.log`). The earlier narrow Clippy also exited 0.
- Workspace all-target check: exit 0 (`workspace-check.log`).
- Workspace format check: exit 0 (`fmt-check.log`).

Cargo printed diagnostic text about unrendered template manifests in the
existing `workers-rs` dependency checkout. Those diagnostics also preceded
successful narrow runs; each final gate above exited 0. That dependency
checkout was left untouched.

The sensitivity build removed startup file discovery and refused Register when
a lifecycle already existed. The separate Register mutation is necessary because
a committed-answer replay does not rely on driving due work at startup.
Sensitivity is not tests-first proof of a product defect.

Commands:

```sh
cargo test -p collaboration-central-tokio --features conformance --test process_restart -- --nocapture
cargo clippy -p collaboration-central-tokio --features conformance --test process_restart -- -D warnings
cargo test -p collaboration-central-tokio --features conformance --all-targets --no-fail-fast
cargo clippy -p collaboration-central-tokio --features conformance --all-targets -- -D warnings
cargo check --workspace --all-targets
cargo fmt --all -- --check
```

The final process-test executable was SHA-256
`d69b91a2b5ea633d95b221a38561083caa65ce283c4f6d1c2c2b12da54a63f34`.
Its path and size are in `executable.json`; compiler versions are in
`rustc.log` and `cargo.log`. The real-path integration tests supply the
manual-process proof as well; no equivalent extra manual run was performed.

## Limits

This proves the three selected native local scenarios with fixture Access/JWKS.
It does not prove Worker recovery, production process replacement, hosted or
staging behavior, Router writer/outbox recovery, HTTP import, the full delivery
or acceptance. The Lead commissions Verifier B separately on the declared
commit; no independent observation has arrived in this lane.

The new Rust files follow the existing integration-test conventions. Raw SQL
is read-only test evidence against named columns; no production query, schema,
migration or checked-query metadata changes. Opaque call/HTTP envelopes use
`serde_json::Value` at their serialization boundary; requests, frames and
answers otherwise use the existing domain types. No domain rule or ownership
boundary changes.
