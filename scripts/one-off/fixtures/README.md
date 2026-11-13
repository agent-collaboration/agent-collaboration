# Synthetic released-root fixture

`released-0.1.125/state.sqlite` and its WAL/SHM were produced by
`AsyncSqliteStateStore` from the real release `v0.1.125`, commit
`cbf56bad640ae981900c646ff2ae0b64aaaebd3b`. The adjacent standalone builder pins
that exact source, writes a disabled synthetic account, quota and session affinity,
then exits without closing the writer. Committed account rows remain in the WAL.
There are no credentials or personal data. Tests copy the fixture before opening it.

From the repository root, regenerate into a fresh destination:

```sh
cargo build --locked --manifest-path scripts/one-off/fixtures/0.1.125-builder/Cargo.toml --target-dir target
scripts/cargo_debug_signing_runner.sh target/debug/router-root-0125-fixture-builder /absolute/fresh-output
```

The builder refuses an existing destination. Keep the files together. The TypeScript
cases add disposable old settings, credential-lock bytes, identity/interaction
markers and collaboration files; those markers exercise backup-only classification,
not an old board-schema import. All existing proxy tables and rows are compared
before and after conversion, independently of the converter's plan.
