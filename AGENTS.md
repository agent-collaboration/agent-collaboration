# AGENTS.md

## Module Cycle Ratchet

With cargo-modules 0.28.0 installed, run `cargo nextest run -p workspace-architecture-check --run-ignored only`.
After reducing cycles or their edges, update `architecture/module-cycle-baseline.json` with `ARCHITECTURE_BLESS=1 cargo nextest run -p workspace-architecture-check --run-ignored only`; blessing an existing baseline refuses growth.

## Languages

- No Python in this repository. Write code in TypeScript or Rust; for any
  other language, ask the owner first.

## Verification Tooling

Tests must not depend on wall-clock time: wait on a causal signal or an injected clock; a timeout may only bound a causal wait.

- Install tooling dependencies with `pnpm --dir verification install --frozen-lockfile`.
  Run `check`, `lint`, `format:check`, and `test` through that package.
- The isolated Tailnet Host driver is a verification-only Rust executable:
  `cargo build -p codex-router-tailnet-verification-host` and
  `target/debug/codex-router-tailnet-verification-host host "$HOST_PLAN"`.
  Follow its [runbook](crates/tailnet-verification-host/README.md) and the
  verification skill's declared-target and allocation requirements.
- Check that driver with `cargo test -p codex-router-tailnet-verification-host`
  and `cargo clippy -p codex-router-tailnet-verification-host --all-targets -- -D warnings`.

## Release And Homebrew Distribution

- Before merging a user-facing code change, advance the workspace package
  version to the next unused semantic version and update `Cargo.lock`. Do not
  merge releasable code while leaving Homebrew on an older version.
- After the versioned change reaches `main`, publish the matching `v<version>`
  tag and require `.github/workflows/release.yml` to publish the GitHub release
  artifact, update `ShravanSunder/homebrew-taps`, and verify the Homebrew
  installation.
- The release workflow's tap job waits for a second macOS runner. Once its
  build job has published the release asset, run
  `node verification/tooling/publish-homebrew-tap-local.ts` from the repository root on
  an Apple Silicon Mac instead of waiting: it verifies the tag is on `main` and
  the asset matches the release digest, updates the formula, runs the tap
  job's Homebrew checks against a real install, and pushes the tap. The CI tap
  job still runs its own validation when it gets a runner and commits nothing
  if it checks out the tap after the local push; whichever push lands second
  finds identical content. It never restarts the running production Router.
- On development machines, the installed `codex-router`, `agent-collaboration`
  and `agent-sessions` come from the Homebrew tap, the same release artifact
  users get: `brew update && brew upgrade codex-router`. Do not `cargo install`
  these binaries; a copy in `~/.cargo/bin` comes earlier on `PATH` and silently
  shadows the Homebrew one. Exercise unreleased code through `cargo run`,
  tests, or an isolated debug Host, not by installing it.
- Released executables are signed with the Developer ID Application identity
  of team `974QD84WVC`, the hardened runtime and a secure timestamp, under the
  fixed identifiers `dev.shravansunder.<executable>`. Keychain approvals belong
  to that identity, so they survive upgrades; a linker-signed build is
  identified by its hash and prompts again after every build. The release job
  imports the certificate from the `APPLE_CERTIFICATE_BASE64` and
  `APPLE_CERTIFICATE_PASSWORD` secrets and fails without them; it does not
  notarize, because Homebrew installs the tarball without quarantine.
- Local Apple Silicon `cargo run` and `cargo test`, in any profile, go
  through `scripts/cargo_debug_signing_runner.sh`, which signs every
  executable it runs with the same certificate, without a secure timestamp so
  it works offline: `codex-router`, `agent-collaboration`, `agent-sessions`,
  `codex-router-board-upgrade` and `board-debug-runtime` under
  `dev.shravansunder.<executable>.debug`, and test, example and helper
  binaries under `dev.shravansunder.test.<name>`. Never execute a freshly
  built binary unsigned: macOS verifies each new ad-hoc-signed executable,
  and nextest launches every test binary. Debug and released builds are different code identities on
  purpose: the debug Router keeps its own Keychain item, and approvals given
  to one never apply to the other. A debug build another process starts
  directly, such as the `--router-binary` a debug-host example launches,
  bypasses the runner; sign it first with
  `scripts/cargo_debug_signing_runner.sh --sign-only <executable>...` (any
  built executable).
  When signing cannot happen (no certificate in the login keychain, a locked
  keychain) the runner runs the build linker-signed and says so.
- Keep release publication separate from production process replacement.
  Publishing or installing a new binary never authorizes restarting the
  running production router.

## Local Debug Boundaries

- Never stop, restart, or replace the production Codex router process unless
  the user explicitly says: "replace the production Codex router process".
  Installing a binary does not mean replacing the running process.

- Codex session state is normal Codex state. `codex-router sessions` reads
  `$HOME/.codex/state_5.sqlite` and `$HOME/.codex/sessions/*.jsonl` read-only;
  it must not redirect to a repo-local fake Codex home in debug builds.
- Router-owned runtime state is separate. Debug `cargo run -p codex-router-cli`
  defaults router state/secrets to `$HOME/.codex-router-debug`; installed or
  home-default runs use `$HOME/.codex-router`.
- The debug Codex profile lives in normal Codex home as
  `$HOME/.codex/codex-router-debug.config.toml` and points Codex at the debug
  router port. Keep this profile/config separate from router-owned state.
- `CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET` is a debug-build-only override for
  real app-server acceptance. It must be an absolute socket path inside a
  dedicated owner-controlled directory because upstream Codex makes the socket
  parent private. It isolates the endpoint, not Codex home: upstream still
  serializes app-server startup per `CODEX_HOME`.

## Cargo Build Storage

- Periodically check `target/debug` size with `du -sh target/debug`, especially
  after dependency/toolchain changes or extensive build and test runs.
- When retained artifacts have grown substantially, run
  `cargo clean --workspace --profile dev` from the repository root. First
  confirm no active builds, tests, or running debug processes depend on those
  artifacts; defer cleanup if they do. Use `--dry-run` to preview removal.
- This removes rebuildable workspace debug artifacts, not account data or
  credentials. Subsequent builds may take longer. Do not clean after every
  build or stop production processes to make cleanup possible.
- During development, default to `cargo check -p <package>` and the narrowest
  relevant `cargo test -p <package> <test>`. Reserve workspace-wide builds and
  tests for the required completion gate; preserve every required gate.

## Terminal UI Layout

- Build every terminal UI with iocraft layout primitives. Use nested
  `View`s, flex growth/shrink, gaps, margins, padding, and separate `Text`
  children for alignment and spacing. Do not simulate layout with manually
  padded formatted strings or terminal-filling child panels.
- Keep navigation, content, flexible empty space, and bottom shortcuts as
  distinct iocraft siblings. Use a flex-growing spacer to pin shortcuts to the
  bottom while allowing detail panels to remain content-sized.

## SQLite And Rust Validation

- Use SQL CHECK constraints only for boolean storage (`0` or `1`). Do not
  encode enum/tag membership, string lengths, numeric ranges, or variant
  cross-field rules in database CHECK constraints.
- Enforce those rules through Rust enums, Serde, and validated domain
  constructors, both on incoming requests and when decoding stored rows.
  Reject invalid stored values explicitly; never silently coerce them.
- Keep primary keys, foreign keys, NOT NULL constraints, and unique indexes
  for relational integrity. SQLx checked queries validate SQL/schema
  compatibility; they do not replace domain validation.
- Keep evolving domain rules out of table definitions to avoid unnecessary
  SQLite table rebuilds. This rule does not authorize removing existing
  constraints or rewriting migrations outside the requested scope.

## Servers And HTTP

- `codex-router-proxy` uses hyper and tungstenite directly (`hyper`,
  `hyper-util`, `hyper-rustls`, `hyper-tungstenite`, `tokio-tungstenite`) for
  fast pass-through of model traffic and websockets. The harness connections
  (Codex app-server and native control, ACP providers, Claude Code peer
  messaging) also use `tokio-tungstenite` directly. Work on other surfaces
  does not change the proxy or the harness connections.
- Every other server uses axum, HTTP clients use reqwest, and MCP uses rmcp
  mounted on axum.

## Upstream Workarounds

- Log each fix we carry for an upstream library's defect in
  `docs/upstream-workarounds/`, with its evidence, how to recreate it, and the
  check that says it can be removed.
- Fork an upstream dependency, or pin a fork of one, only with the owner's
  explicit approval.
