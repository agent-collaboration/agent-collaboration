These synthetic stores contain no real board data.

`populated-root/` is a Router root written by `../root-fixture-builder`, whose
store writers are pinned to Router `42a5ff53efe1f7cbd690b1ab3e2689e526355f51`,
the last layout before `agent-router-update`. Its `agent-communication/` holds
`fleet-outbox.sqlite` with a pending request, a request decided at a fixed
time and a pending project creation with its input (all only in `-wal`),
`board-cursors.sqlite` with one signing key, and `fleet-cache.sqlite` with one workspace page checkpointed into
the database file and a second committed page left only in `-wal`, because the
builder exits without closing the store. Keep each `-wal` file beside its store.
The journey `the_fixture_keeps_one_committed_page_only_in_the_write_ahead_log`
checks that property before any update case relies on it.

Regenerate from the repository root (the builder refuses an existing output):

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo run --manifest-path crates/agent-router-update/tests/root-fixture-builder/Cargo.toml --target-dir target -- crates/agent-router-update/tests/fixtures/populated-root
```
