# CI and dependencies

These shared manifests and workflow files also serve the rest of the repository. Current presence is not evidence that the native verifier dependency or gate remains. The inventory distinguishes unchanged base files from surviving branch changes.


## ci.yml

- **Path:** [.github/workflows/ci.yml](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/.github/workflows/ci.yml)

- **Now:** Present, unchanged from #173.

- **Purpose now:** Runs the repository validation workflow; this branch preserves the #173 workflow unchanged.

## Cargo.lock

- **Path:** [Cargo.lock](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/Cargo.lock)

- **Now:** Present, unchanged from #173.

- **Purpose now:** Locks Rust dependency resolution; this branch preserves the #173 lockfile unchanged.

## Workspace root/Cargo.toml

- **Path:** [Cargo.toml](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/Cargo.toml)

- **Now:** Present, unchanged from #173.

- **Purpose now:** Defines the Rust workspace, shared dependencies and lint policy; no branch changes survive in this file.

## acp-client-runtime/Cargo.toml

- **Path:** [crates/acp-client-runtime/Cargo.toml](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/acp-client-runtime/Cargo.toml)

- **Now:** Present, unchanged from #173.

- **Purpose now:** Declares runtime dependencies/features; no branch changes survive in this manifest.

## codex-router-host/Cargo.toml

- **Path:** [crates/codex-router-host/Cargo.toml](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/codex-router-host/Cargo.toml)

- **Now:** Present, unchanged from #173.

- **Purpose now:** Declares Host dependencies/features; no branch changes survive in this manifest.
