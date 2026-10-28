# Generated query headers and the Clippy baseline

`fleet-sync-query-gen` v0.3.0 emits
`#![allow(clippy::manual_async_fn)]` in every generated query module. The
workspace's `clippy::allow_attributes_without_reason` deny rejects that header.

The 16 generated-module declarations in
`crates/collaboration-central-core/src/lib.rs` expect only
`clippy::allow_attributes_without_reason`, with a reason identifying the
upstream generator. Generated Rust, query manifests, metadata, and `build.rs`
freshness checks remain unchanged. The exception does not suppress lint
findings in handwritten code.

To reproduce, remove one declaration's expectation and run
`cargo clippy -p collaboration-central-core --lib -- -D warnings`.
Editing the generated header instead fails `verify_manifest` with
`Stale("src/generated_forwards.rs")` (or the corresponding generated module).

Remove these expectations when the pinned upstream generator emits reasoned
headers or no longer needs the allowance. Regenerate through the generator,
retain its normal freshness checks, and require
`cargo clippy --workspace --all-targets -- -D warnings` to pass without
unfulfilled expectations.
