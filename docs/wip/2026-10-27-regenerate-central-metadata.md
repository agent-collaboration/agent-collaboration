# Regenerate central metadata for fleet-sync 0.4.0

Continue [PR #180](https://github.com/agent-collaboration/agent-collaboration/pull/180) on the Lead-allocated Sunclaw machine. Run on a Mac only after the Lead explicitly relays the owner's permission. The Mac generator stop remains in force until then. Do not merge, tag, restart the production Router, change SQL, increase timeouts, or disable the freshness gate.

Plan identity: none. Authority: the Lead's bounded request to regenerate central checked-query metadata after the release-tag switch.

## Prerequisites and current proof

- Router branch: `fix/fleet-sync-0.4`; code prerequisite: `3250eee22373ffaa7b64eaf8d7101912c722992a`. Fetch the latest branch, which also contains this recipe; the prerequisite commit must be its ancestor. Use a clean checkout with no concurrent writer.
- Upstream: `https://github.com/agent-collaboration/agent-fleet-sync.git`, annotated tag `v0.4.0`, peeled commit `b9da4b5f230f81430fdf1140fbb634cf64decd8e`.
- Rust 1.98.1, Cargo, Git, and Bash. Both repositories pin Rust 1.98.1. macOS additionally requires a usable Developer ID signing identity through the repository's runner.
- All 27 dependency pins and both lockfiles already use the release tag. Keep those inputs fixed before generation. The generator records its own workspace `Cargo.lock` and source hashes in central's metadata.
- Earlier workspace check and clippy exited 101 at `collaboration-central-core/build.rs`, reporting `Stale("generator:workspace/Cargo.lock")`; fmt exited 0. Those results precede the source-identical tag switch. The generator has been built on the Mac but has never been executed there. The commands below were checked against the CLI source; they have not been run as an end-to-end recipe.

Every code block below runs in **Bash from the Router repository root**, with `set -euo pipefail` enabled. Stay at that root; commands target other directories explicitly. Do not stash, reset, rebase, or overwrite existing work.

```bash
set -euo pipefail
test -z "$(git status --porcelain)"
test "$(git branch --show-current)" = fix/fleet-sync-0.4
git fetch origin fix/fleet-sync-0.4
git merge --ff-only origin/fix/fleet-sync-0.4
git merge-base --is-ancestor 3250eee22373ffaa7b64eaf8d7101912c722992a HEAD
```

## Build the pinned generator

The clone and build output stay in task scratch. Reuse an existing clone only if the identity and clean-status checks pass. Do not modify the upstream checkout or regenerate its lockfile.

```bash
set -euo pipefail
mkdir -p tmp/fleet-sync-0.4
if [ ! -d tmp/fleet-sync-0.4/upstream/.git ]; then
  git clone --depth 1 --single-branch --branch v0.4.0 \
    https://github.com/agent-collaboration/agent-fleet-sync.git \
    tmp/fleet-sync-0.4/upstream
fi
test "$(git -C tmp/fleet-sync-0.4/upstream rev-parse HEAD)" = b9da4b5f230f81430fdf1140fbb634cf64decd8e
test "$(git -C tmp/fleet-sync-0.4/upstream rev-parse 'v0.4.0^{}')" = b9da4b5f230f81430fdf1140fbb634cf64decd8e
test -z "$(git -C tmp/fleet-sync-0.4/upstream status --porcelain)"
cargo build --locked \
  --manifest-path tmp/fleet-sync-0.4/upstream/Cargo.toml \
  -p fleet-sync-query-gen --features native \
  --target-dir target/fleet-query-generator
```

The executable is `target/fleet-query-generator/debug/fleet-sync-query-gen`. Building does not run it.

## Sign before execution

On non-macOS platforms, the repository runner's `--sign-only` returns without signing. On macOS, the runner at prerequisite head `3250eee2` rejects helper executables. Use an unchanged scratch copy of the all-executable runner from PR #179, commit `f0e66257f23c82ba535952310ffc21802cdb60b0`. This does not change a tracked Router file.

```bash
set -euo pipefail
if [ "$(uname -s)" = Darwin ]; then
  git fetch origin pull/179/head
  mkdir -p tmp/fleet-sync-0.4/signing/scripts
  git show f0e66257f23c82ba535952310ffc21802cdb60b0:scripts/cargo_debug_signing_runner.sh \
    > tmp/fleet-sync-0.4/signing/scripts/cargo_debug_signing_runner.sh
  bash tmp/fleet-sync-0.4/signing/scripts/cargo_debug_signing_runner.sh \
    --sign-only target/fleet-query-generator/debug/fleet-sync-query-gen
  codesign --verify --strict target/fleet-query-generator/debug/fleet-sync-query-gen
  codesign -dv target/fleet-query-generator/debug/fleet-sync-query-gen
else
  bash scripts/cargo_debug_signing_runner.sh \
    --sign-only target/fleet-query-generator/debug/fleet-sync-query-gen
fi
```

On macOS, inspect the signing output before proceeding: require a Developer ID signature, not `Signature=adhoc`. If the runner says the binary stays linker-signed, stop and return that signing gap to the Lead without launching it. Signing permission does not lift the owner's Mac execution stop.

## Regenerate all 16 corpora

This list comes directly from `crates/collaboration-central-core/build.rs`. Each manifest names the schema, SQL, metadata file, and generated Rust output. The generator applies the migrations to its scratch store and describes the statements; it does not connect to production state.

```bash
set -euo pipefail
for fleet_manifest in \
  forward-queries.json \
  admin-import-queries.json \
  checked-queries.json \
  bootstrap-queries.json \
  bootstrap-probe-queries.json \
  enrollment-queries.json \
  fleet-record-read-queries.json \
  fleet-record-write-queries.json \
  fleet-repository-queries.json \
  fleet-feed-queries.json \
  copy-summary-queries.json \
  fleet-lease-queries.json \
  fleet-archive-queries.json \
  registry-lease-queries.json \
  machine-notice-queries.json \
  session-conformance-queries.json
do
  target/fleet-query-generator/debug/fleet-sync-query-gen \
    --manifest "crates/collaboration-central-core/$fleet_manifest"
done
```

Expected tracked changes are the following 16 files, all under `crates/collaboration-central-core/`:

| Manifest | Metadata file |
| --- | --- |
| `forward-queries.json` | `forward-query-metadata.json` |
| `admin-import-queries.json` | `admin-import-query-metadata.json` |
| `checked-queries.json` | `checked-query-metadata.json` |
| `bootstrap-queries.json` | `bootstrap-query-metadata.json` |
| `bootstrap-probe-queries.json` | `bootstrap-probe-query-metadata.json` |
| `enrollment-queries.json` | `enrollment-query-metadata.json` |
| `fleet-record-read-queries.json` | `fleet-record-read-query-metadata.json` |
| `fleet-record-write-queries.json` | `fleet-record-write-query-metadata.json` |
| `fleet-repository-queries.json` | `fleet-repository-query-metadata.json` |
| `fleet-feed-queries.json` | `fleet-feed-query-metadata.json` |
| `copy-summary-queries.json` | `copy-summary-query-metadata.json` |
| `fleet-lease-queries.json` | `fleet-lease-query-metadata.json` |
| `fleet-archive-queries.json` | `fleet-archive-query-metadata.json` |
| `registry-lease-queries.json` | `registry-lease-query-metadata.json` |
| `machine-notice-queries.json` | `machine-notice-query-metadata.json` |
| `session-conformance-queries.json` | `session-conformance-query-metadata.json` |

Each manifest also rewrites its declared `src/generated_*.rs` output. Those outputs should remain byte-identical because the SQL and query descriptions have not changed. Inspect the diff: expected changes are generator/driver source hashes, including `generator:workspace/Cargo.lock`. If query descriptions, generated Rust, SQL, manifests, or lockfiles change, stop and return the exact diff to the Lead before committing. Do not hand-edit hashes to make verification pass.

## Verify freshness and required gates

```bash
set -euo pipefail
git diff --stat
git diff --check
git diff --exit-code -- 'crates/collaboration-central-core/src/generated_*.rs'
git diff --exit-code -- Cargo.lock scripts/verification/board_upgrade_native/Cargo.lock
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

The exact previously failing command, `cargo check --workspace --all-targets`, must change from exit **101** with the freshness error to exit **0**. Its central build script verifies all 16 corpora. Require clippy and fmt to exit 0 as well. Record each command and exit code; stop on any failure and return its log. These gates do not establish production CPU or E2E behavior. Do not run tests or launch a Host under this assignment.

## Commit and hand back

After checking that only the 16 listed metadata files changed:

```bash
set -euo pipefail
git add -- 'crates/collaboration-central-core/*query-metadata.json'
git diff --cached --stat
git diff --cached --check
git commit -m 'build: regenerate central SQL metadata for fleet-sync 0.4.0'
git push origin fix/fleet-sync-0.4
git rev-parse HEAD
```

Follow repository signing policy; after two failed 1Password signing attempts, use `git commit --no-gpg-sign` with the same message. Do not amend or force-push. Do not commit scratch clones, binaries, signing helpers, or logs.

Reply to the Lead with PR #180's new head, changed files, all three gate exit codes, generator/signing results, and any unverified behavior. The Lead owns further verification and acceptance.
