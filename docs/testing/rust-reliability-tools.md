# Rust feedback and diagnostic tools

Use the repository's Rust toolchain. Bootstrap exact tool versions without
replacing globally installed tools:

```sh
scripts/tooling/bootstrap-tools.sh ci
export PATH="$PWD/tmp/rust-tools/bin:$PATH"
cargo nextest run -p collaboration-protocol
cargo nextest run --profile ci --workspace --features codex-router-cli/keychain-test-support
```

Enable `keychain-test-support` for both compilation and execution. The compiled
CLI acceptance child is a normal binary rather than a `cfg(test)` target, so
without this feature it can reach the production macOS Keychain despite using a
temporary router root.

The CI profile reports all failures, disables retries and reports tests exceeding
60 seconds. It does not terminate valid long-running integration tests; their
existing deadlines remain in force. Focused runs are feedback, not full workspace
proof. Doc tests run separately with `cargo test --workspace --doc`.

Versions live in `scripts/tooling/tool-versions.tsv`. Upgrade deliberately by
editing the declaration and rerunning bootstrap and affected checks. Bootstrap
validates binaries after cache restoration and installation. Audit data stays
current even when the executable version is pinned.

## Explicit diagnostics

```sh
scripts/tooling/bootstrap-tools.sh diagnostic
rustup component add llvm-tools-preview
scripts/tooling/rust-diagnostics.sh mutants codex-router-selection crates/codex-router-selection/src/eligibility.rs
scripts/tooling/rust-diagnostics.sh coverage codex-router-selection
```

Run diagnostics while other Cargo jobs are idle. Mutation testing is limited to
one selected source file, one worker, a 300-second build timeout and a 60-second
test-run timeout. It uses
scratch copies rather than modifying the checkout. A timeout or surviving mutant
is a finding to inspect, not proof that a test is missing; tool failures are not
successful reports. No blanket score gate is imposed.

Coverage has its own target directory, so tool cleanup does not discard ordinary
build output. Reports describe executed lines/regions under the selected package;
they do not prove assertion strength or concurrency correctness. Subprocesses
count only if separately instrumented and their profiles collected. These commands
do not claim branch or doc-test coverage. Cargo network offline mode differs from
SQLX_OFFLINE, which diagnostic builds explicitly preserve.

Keep any discovered defect grounded in an existing contract. Diagnostic tools do
not replace real database migration, process/socket/PTY or runtime acceptance tests.

## Checked queries by database engine

Use the Node and pnpm versions pinned in `verification/package.json` and install
tooling dependencies with `pnpm --dir verification install --frozen-lockfile`.

Stock SQLite query metadata is generated against separate disposable schemas for
account, automation and provider-operation queries:

```sh
scripts/tooling/bootstrap-tools.sh sqlx
node verification/tooling/prepare-sqlx.ts
node verification/tooling/prepare-sqlx.ts --check
```

Turso query metadata is generated through the native driver from the project,
fleet and Router-local migration sets, plus the driver's own test schema:

```sh
node verification/tooling/prepare-sqlx-turso.ts
node verification/tooling/prepare-sqlx-turso.ts --check
```

The wrappers supply their own database URLs and remove their temporary schema
databases when finished. They never read Router or Codex home. Stock metadata
lives in the root `.sqlx`; Turso metadata lives in each querying package's `.sqlx`.
`message-board-storage` has only native Turso queries. When stock preparation
passes a SQLite URL through its dependency graph, that package's build script
uses its existing native cache. The separate Turso preparation still describes
every native query online against a fresh native schema and checks the cache.
Both full `--check` commands are required. Run metadata generation before dependent
workspace builds, since stock generation replaces the shared root cache as it runs.
Commit metadata changes with query/schema changes. Metadata checks do not replace
real migration tests.
