# UW-1: Turso's Rust binding spins while waiting on a lock

**Status:** in place (decided 2026-10-11; verified 2026-10-13)
**Lives in:** `crates/sqlx-turso-core`: connection setup (`engine_connection.rs`, `connect_options/`), timer budget (`busy_wait.rs`), statement stepping (`executor/`) and transaction cleanup (`connection.rs`)

## In one paragraph

When a statement has to wait for another connection's write lock, Turso's engine returns "sleep for this long", but it wakes the task first. The SDK then throws the duration away, and the `turso` crate's `Future` returns `Pending` with no timer. The task is polled again straight away, millions of times, for the whole wait. On this project's Macs, a 2-second lock wait used about 2.9 s of CPU and about 20 million polls. Turso's own maintainers describe the fix in their issue: sleep the way SQLite does.

## Upstream

- Turso issue [#7211](https://github.com/tursodatabase/turso/issues/7211), "Replace step() busy-poll with sleep". Opened by a Turso maintainer on 2026-05-21; open, with no comments, as of 2026-10-11.
- Turso PR [#7215](https://github.com/tursodatabase/turso/pull/7215), an outside fix. A bot closed it automatically on 2026-05-21 because the contributor's trust score was too low. It was never reviewed on its merits.
- Checked on 2026-10-11 at Turso `main` (`120511cb975f514fa66ec8f40c897fc19c64d88f`) and in the releases 0.8.1, 0.8.2 and 0.8.3-pre.1. The relevant files are byte-identical across those releases:
  - `core/statement.rs`: `waker.wake_by_ref()` runs before `StepResult::Sleep { duration }` is returned;
  - `sdk-kit/src/rsapi.rs`, in `step_inner`: `IO | Yield | Sleep { .. }` all map to `Io`, and the duration is dropped;
  - `bindings/rust/src/lib.rs`: on `Io` it calls `run_io()`, then returns `Poll::Pending`.
- The SDK's `async_io = false` mode doesn't help either: it spins the same way, just on the calling thread.

## Evidence

Measured 2026-10-11 on an Apple Silicon Mac (macOS 26.5.2, rustc 1.98.1, Turso 0.8.1 pinned, Tokio multi-thread runtime with two workers). One thread holds `BEGIN IMMEDIATE` for 2 s while the other inserts with `busy_timeout = 5 s`:

| Case | Wall | Process CPU | Polls |
|---|---|---|---|
| no contention | 2.00 s | 0.02 s | — |
| Turso async binding (the spin) | 2.03 s | 2.86–2.91 s | 19.8–20.5 million |
| the same wait, honouring `Sleep { duration }` with `tokio::time::sleep` | 2.06 s | 0.02–0.04 s | — |
| `busy_timeout = 0` plus a Tokio sleep backoff (earlier probe, 2026-10-07) | ~2 s | 0.018 s | — |

## What we do

- Our driver opens every connection with `busy_timeout = 0`. Turso then reports "busy" at once instead of waiting inside itself.
- On "busy", the driver awaits `tokio::time::sleep`, growing the delay from 5 ms to a 50 ms cap. Each statement keeps the configured wait budget (5 s by default). `BusySnapshot` is returned immediately because waiting cannot make that transaction writable.
- Prepared queries resume the current native row step. Batches use the pinned Turso parser's offsets to execute each statement's original SQL text. A busy statement is retried without repeating the successful prefix. Arguments remain unsupported for batches, and the statement cache is forgotten before a batch starts.
- Router starts writes with `BEGIN IMMEDIATE`. Transaction commands, native prepares and schema-cookie reads also use the driver's timer budget.
- SQLx pool return calls `ping` before making a connection idle. The driver flushes its queued dropped-transaction rollback there, so an idle pooled connection releases its write lock before another writer enters.
- Permanent native tests in `tests/busy_lock_waits.rs` prove the two-second low-CPU wait, the five-second busy deadline and connection reuse, failed-prefix preservation, current-statement batch retry and independent writer entry after a pooled transaction is dropped. Executor tests also cover Unicode, escaped literals, comment and trigger-body semicolons, and a later parse error retaining an executed prefix.

On 2026-10-13 the two-second contended BEGIN used 24,043 µs of process CPU (2.058 s wall), against 2,025,659 µs before the repair. The contended batch used 13,004 µs. All five lock/pool tests and 74 driver library tests passed; the full feature suite also passed its checked-query, migration and dependency tests. No Turso engine change or Cargo profile change is carried.

## How to recreate

1. Create a scratch crate pinned to `turso = "=0.8.1"` with `default-features = false`, using a file database in a temp directory.
2. Thread A: on a core connection, run `BEGIN IMMEDIATE`, insert a row, sleep 2 s, `COMMIT`.
3. Task B, on a Tokio multi-thread runtime, using the `turso` crate: `busy_timeout = 5000`, then `INSERT` while A holds the lock. Count polls with a wrapper `Future` and read `getrusage(RUSAGE_SELF)` before and after.
4. The spin shows as process CPU about equal to or above wall time, with millions of polls. With the fix, CPU stays under 0.1 s.

## Remove or revisit when

- Upstream #7211 closes with a merged fix, or we upgrade Turso. On upgrade:
  - check the `Io` branch in `bindings/rust/src/lib.rs` (or the published `turso/src/lib.rs`): it must wait on a timer for busy delays instead of waking itself immediately;
  - re-run the recreation above. If B's CPU stays under 0.1 s with `busy_timeout = 5000` and no driver backoff, delete our backoff and restore the normal busy timeout.

## History

- 2026-10-07: first seen while porting the driver (probe: 2.03 s CPU for a 2.03 s wait).
- 2026-10-11: re-verified at Turso main `120511c` and in releases up to 0.8.3-pre.1; measured as above; the owner chose a driver-side fix with no change to Turso and no upstream PR.
- 2026-10-13: timer waits, original-text per-statement batch execution and pool-return rollback cleanup verified on pinned Turso 0.8.1. The upstream removal check remains required on upgrade.
