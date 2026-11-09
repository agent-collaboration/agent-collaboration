# UW-13: Turso 0.8.1 native publication checkpoint

The publication owner runs `PRAGMA wal_checkpoint(TRUNCATE)` before closing its connection and replacing a live file. The shared query generator refuses this statement because the owned driver describes `busy`, `log` and `checkpointed` as `NULL` types. The generator has no result-type override.

This one host maintenance query uses a runtime-checked adapter, authorized by the original fleet design decision 40. It lives in [`checkpoint_for_publish`](../../crates/fleet-project-writer/src/native_file.rs), with the SQL in [`wal_checkpoint.sql`](../../crates/fleet-project-writer/sql/wal_checkpoint.sql). [`native_publish.rs`](../../crates/fleet-tokio-objects/src/native_publish.rs) re-exports and calls it before closing the connection and entering the publication callback.

The adapter requires exactly one row and exactly the named columns `busy`, `log` and `checkpointed`. `busy` must be a nonnegative integer. On success, both counters must be nonnegative integers and equal. A busy or incomplete checkpoint refuses publication retryably; the rename callback does not run. Other result type or shape changes produce `InvalidCheckpoint`.

Turso 0.8.1's failure branch writes only `busy = 1`, leaving both counters NULL (`turso_core` 0.8.1, `vdbe/execute.rs:799-806`). The adapter accepts NULL counters only when busy is nonzero. Every other fixed runner query stays on its owner's checked SQL path.

## Evidence and recreation

The real-file controls in [`native_publish_tests.rs`](../../crates/fleet-tokio-objects/src/native_publish_tests.rs) cover both paths:

- `checkpoint_closes_before_rename_and_reopens_the_published_file` checkpoints and closes the replacement, publishes it, and reads its actual contents after reopening.
- `held_native_reader_refuses_publish_and_leaves_the_old_file_available` holds a reader across a committed write. It observes the engine's actual `busy = 1` and NULL counters, proves the rename callback was not entered, ends the reader, and retries publication successfully. It does not fabricate checkpoint rows.

Run from the repository root:

```sh
cargo nextest run --locked -p fleet-tokio-objects --lib -E 'test(native_publish_tests::)'
```

The original evidence and authorization moved from [agent-fleet-sync at c59399e](https://github.com/agent-collaboration/agent-fleet-sync/blob/c59399e15e999b27a27970c1c4ec1644e1a8269e/docs/upstream-workarounds/turso-0.8.1-checkpoint.md). No upstream issue was filed in that record.

## Removal check

At the next native driver or engine upgrade, re-check checkpoint description and the actual held-reader result. Replace the runtime adapter with the normal checked-query path only when the generator accepts its real result types and both real-file controls pass through that path. Preserve the busy refusal and callback exclusion if the engine still returns NULL counters. A successful checkpoint alone does not close this check.

These controls do not establish snapshot verification, local-state carry, install-ID replay or process-kill recovery; those remain obligations of the complete snapshot-install journey.
