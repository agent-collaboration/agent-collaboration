# UW-16: rust-cache keys include unused runner toolchains

Status is in place. Affected action is `Swatinem/rust-cache` v2 at
`d1527c8f67ef9af357290475d4af4b0fab8f12e0`.

## Evidence and boundary

The action fingerprints the active compiler with `rustc -vV`, then fingerprints
every entry in `rustup toolchain list --quiet`. All of those compiler identities
feed the restore-prefix hash alongside the selected environment variables.
This includes the extra compiler preinstalled on the hosted runner, even when
CI builds with its pinned toolchain. The action appends a separate dependency
hash to make the full cache key. [Pinned compiler enumeration](https://github.com/Swatinem/rust-cache/blob/d1527c8f67ef9af357290475d4af4b0fab8f12e0/src/config.ts#L390-L430) · [Pinned key construction](https://github.com/Swatinem/rust-cache/blob/d1527c8f67ef9af357290475d4af4b0fab8f12e0/src/config.ts#L97-L162)

The Rust local end-to-end job used Rust `1.98.1` and dependency hash `415577a5`
in both runs. Its `Cache Configuration` changed from extra Rust `1.99.1` and
restore suffix `4d1f5b38` to extra Rust `1.100.0` and suffix `144fef9b`. The hash
replay reproduced both suffixes with the same environment values. The two
prefixes let full-size archives accumulate despite unchanged build inputs.
[Earlier E2E run](https://github.com/agent-collaboration/agent-collaboration/actions/runs/44553581060/job/135663514087) · [Later E2E run](https://github.com/agent-collaboration/agent-collaboration/actions/runs/44563490162/job/135698836009)

## Local correction

The CI workflow supplies a `cmd-format` that returns only the workflow-selected
`RUST_TOOLCHAIN` for `rustup toolchain list --quiet`. The formatter forwards every
other command, including `rustc -vV` and Cargo metadata. The action still hashes
the active compiler, build environment and dependencies. [Command formatter](../../scripts/tooling/ci-rust-cache-command.sh) · [CI workflow](../../.github/workflows/ci.yml)

## Cache retention

A save can supersede older `v1` keys under its own restore prefix. Restore-only
jobs can still use another prefix without saving an entry. The pruner preserves
those keys because GitHub's `last_accessed_at` field provides no per-run reader
record or freshness guarantee. Retained prefixes can consume budget until
GitHub evicts entries that have gone unaccessed for more than seven days.
[Cache REST API](https://docs.github.com/en/rest/actions/cache?apiVersion=2022-11-28#list-github-actions-caches-for-a-repository) · [GitHub eviction policy](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#usage-limits-and-eviction-policy)

## Recreate

Run two isolated Linux x64 CI jobs on the same checkout with the pinned action.
Give both jobs an initially empty `RUSTUP_HOME` at the same absolute path. In
each, install and select Rust `1.98.1`, then install an extra toolchain without
selecting it. Use `1.99.1` in one job and `1.100.0` in the other. Check that
`rustc -vV` is identical and `rustup toolchain list --quiet` differs. Keep the
cache family, environment and dependency inputs fixed, omit `cmd-format`, and
set `lookup-only: true` and `save-if: false`. Compare `Environment considered`,
`Restore Key`, and `Cache Key` in the two `Cache Configuration` groups. The
affected version lists both compilers and emits different restore keys with
the same dependency suffix.

## Removal check

Repeat the two-job comparison on an upstream upgrade with the formatter absent.
Require identical restore keys and full cache keys across the extra-toolchain
change. Then hold the extra toolchain fixed and change the selected compiler to
verify that the restore key changes. With the compiler fixed, change a Cargo
dependency to verify that the full key changes while the restore key stays the
same. Remove the formatter only after all three comparisons pass.
