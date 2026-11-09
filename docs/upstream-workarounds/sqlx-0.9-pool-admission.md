# UW-14: SQLx 0.9 pool metadata and native file admission

A closed SQLx pool can outlive all its database connections. The fleet writer releases its physical-file claim after the native drain, while closed pool metadata may remain alive.

In pinned `sqlx-core` 0.9.0, `pool/connection.rs::PoolConnection::return_to_pool` keeps an extra `PoolInner` Arc around its asynchronous return and minimum-connection maintenance. `pool/inner.rs::PoolInner::close` waits for connection permits, closes idle connections and sets size to zero. It does not require every Arc or the pool options to disappear. Our [`open_pool`](../../crates/fleet-project-writer/src/replicated_database.rs) callback captures `FileAdmission` in those options. Depending only on the last Arc drop can therefore keep a writer claim after every SDK connection has drained, making an immediate `RetainedFile` open return `FileInUse`.

This is an admission-lifetime mismatch in our code. The [original evidence at c59399e](https://github.com/agent-collaboration/agent-fleet-sync/blob/c59399e15e999b27a27970c1c4ec1644e1a8269e/docs/upstream-workarounds/sqlx-0.9-pool-admission.md) records the post-drain refusal on pre-repair revisions `7bf44196` and `3283e53f`, with real closed pool clones at size zero. The original CI failure did not identify which fallible open failed; that reproduction does not retrospectively identify its call site.

## Fleet workaround

[`NativeFileOwner::close`](../../crates/fleet-project-writer/src/native_file_owner.rs) keeps `OwnerState::Closing` and its claim while draining writer, reader and maintenance pools. After all three closes finish, it calls [`FileAdmission::release_claim`](../../crates/fleet-project-writer/src/file_admission.rs) and becomes closed. The atomic once flag makes subsequent Arc drops harmless, including after a new writer has claimed the same file. A cancelled close retains the claim until a retry completes the drain; ordinary Arc drop remains the fallback for an abandoned owner. Closed pool metadata cannot reopen a closed pool.

Prepared results must also stop owning SDK connections before that drain can establish safe retained admission. The driver-side `TursoStatement::into_metadata` detachment remains in agent-fleet-sync, with its [separate explanation](https://github.com/agent-collaboration/agent-fleet-sync/blob/c59399e15e999b27a27970c1c4ec1644e1a8269e/docs/upstream-workarounds/sqlx-0.9-pool-admission.md#prepared-results-and-the-native-drain) and [statement implementation](https://github.com/agent-collaboration/agent-fleet-sync/blob/c59399e15e999b27a27970c1c4ec1644e1a8269e/crates/sqlx-turso-core/src/statement.rs). That dependency-side fix is not implemented by the fleet owner in this repository.

## Evidence and recreation

Run the permanent checks from the repository root on a Unix host:

```sh
cargo nextest run --locked -p fleet-project-writer --all-features --lib -E 'test(retained_file_lifetime_tests::)'
```

[`retained_file_lifetime_tests.rs`](../../crates/fleet-project-writer/src/retained_file_lifetime_tests.rs) keeps the real connections and file admission in the checks:

- `closed_pool_metadata_does_not_keep_a_drained_writer_claim` holds a checked-out SDK connection while public close is pending. Retained access refuses while that connection can still query. It closes the connection, awaits public close, checks all three pools are closed and empty, then opens the retained file while the database, closed pools and prepared result remain alive. The native read-only challenge and exact file-set comparison distinguish metadata-only survival from a lingering writable SDK connection.
- `dropping_old_closed_pool_metadata_cannot_release_a_new_writer_claim` opens a new writer before dropping old pool metadata and confirms that retained access still refuses.
- Prepared transaction and journal results, and the public row-stream case, cover the driver-detachment interaction. An active stream pins close; its materialized rows may outlive it.

## Removal check

Reassess this binding on an SQLx upgrade. Remove explicit terminal claim release only when the first regression passes with that release removed and closed pool clones still held, while held-connection exclusion, cancelled-close behavior and the later-Arc-drop protection remain correct. An awaited pool close or size-zero observation alone is insufficient. Preserve the real native read-only and file-set checks when reassessing the driver-side detachment. Do not replace these checks with sleeps, retries or a weaker admission barrier. The original record filed no upstream issue.
