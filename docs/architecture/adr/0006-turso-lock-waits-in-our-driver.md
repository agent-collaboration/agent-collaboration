# ADR 0006: Turso is used as published; our driver works around its lock-wait spin

**Status:** accepted, 2026-10-11. **Decided by:** the owner (2026-10-11: no Turso rewrite, no upstream PR, plain `fsync`). **Design:** program design 2, O1 and §9; upstream workaround UW-1.

## Context
Turso's engine can pause at every IO point, but its disk IO isn't asynchronous everywhere:
- On macOS the only backend does blocking syscalls inline. Async disk IO exists only on Linux, with io_uring.
- Its Rust binding spins while waiting on a lock. That's upstream issue #7211, open since May; the only fix was auto-closed.
- Its default "FULL" sync uses plain `fsync`.

## Decision
- **Turso unchanged.** Turso stays exactly as published, pinned.
- **The lock-wait fix lives in the library driver.** The pinned [agent-fleet-sync driver](https://github.com/agent-collaboration/agent-fleet-sync/tree/c414fc5fab95bcffd3333f525b84f45cf997852b/crates/sqlx-turso-core) opens connections with `busy_timeout = 0` and retries "busy" with a Tokio timer backoff, up to 5 s.
- **Retry stays on the current statement.** The pinned Turso parser supplies original SQL boundaries for batches. A successful prefix is never replayed. SQLx pool return flushes queued rollback before making the connection idle.
- **Blocking disk IO is accepted.** It was measured at microseconds per commit, comparable to stock SQLite.
- **Project stores keep plain `fsync`.** Power loss isn't expected, and the data isn't critical yet.

## Consequences
- The owned library driver carries the workaround, logged as UW-1 with a removal check.
- If disk IO ever stalls the Host, the next step is a thread per connection inside our driver.

## Evidence
Measured on 2026-10-11 on an Apple Silicon Mac:
- commits at p50 44 µs and p99 1.2 ms;
- a 2 s lock wait cost about 2.9 s of CPU with the spin, against about 18–40 ms with a real timer.

The source was verified at Turso main `120511c` and in releases 0.8.1 through 0.8.3-pre.1.

The implemented driver was verified on 2026-10-13 against pinned Turso 0.8.1: a 2.058 s contended BEGIN used 24 ms of process CPU; all five busy/deadline/batch/pool tests passed. See [UW-1](../../upstream-workarounds/uw-1-turso-busy-wait-spin.md) for the permanent checks and removal condition.

## Revisit when
Upstream fixes #7211; Host latency shows storage stalls; or the data becomes critical enough to need full flush.
