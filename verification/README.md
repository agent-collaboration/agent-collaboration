# Verification tooling

Run from the repository root with the Node and pnpm versions pinned in
`verification/package.json`:

```sh
pnpm --dir verification install --frozen-lockfile
pnpm --dir verification run check
pnpm --dir verification run lint
pnpm --dir verification run format:check
pnpm --dir verification test
```

`tooling/` owns release formula generation, CI checks, SQLx preparation and proof
receipt validation. `fixtures/` supplies synthetic executable stand-ins for Rust
tests. `observations/` owns declared-runtime observation, process identity and
cleanup, metadata, callbacks and MCP control apparatus. Their original
input/output comparisons are permanent tests; they supply apparatus evidence,
not installed-provider, hosted, release or delivery acceptance.

The isolated Tailnet Host driver is entirely Rust. Build and check
`codex-router-tailnet-verification-host` as described in its
[runbook](../crates/tailnet-verification-host/README.md).

Within `observations/`, choose the journey family: `runtime/` for process
lifecycle, `providers/acp/` and `providers/claude/` for provider observations,
`callbacks/` for typed interactions, `configuration/` for config section
comparisons, `harness/` for evidence reports and `mcp-control/` for MCP Session
control. `shared/` holds common observation contracts. Tests and their original
baselines live with each family.

The board, project, replication, board-upgrade, D1 central mail, cross-machine,
staging and Tailnet board journeys retired with that code in the workspace
storage clean-up; the central-workspace journey replaces them (Verification
Design O2).

Release formula generation is
`node verification/tooling/update-homebrew-formula.ts --help`. The local tap
publisher named in `AGENTS.md` performs external publication and installation;
its unit tests substitute those external commands. Running tests authorizes no
tap push, tag, Homebrew operation or production process replacement.

The quota and compiled TUI apparatus under `scripts/verification/` is assigned
to PR #161 and remains outside this migration's ownership.
