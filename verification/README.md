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
cleanup, metadata, callbacks, and Tailnet peer/readback apparatus. Their original
input/output comparisons are permanent tests; they supply apparatus evidence,
not installed-provider, hosted, release or delivery acceptance.

The isolated Tailnet Host driver is entirely Rust. Build and check
`codex-router-tailnet-verification-host` as described in its
[runbook](../crates/tailnet-verification-host/README.md). The TypeScript Tailnet
peer and readback commands remain under `observations/tailnet/`.

Within `observations/`, choose the journey family: `central/` for D1 central and
mail, `tailnet/` for cross-machine peer/readback, `runtime/` for process lifecycle
and native correlation, `providers/acp/` and `providers/claude/` for provider
observations, `callbacks/` for typed interactions, `configuration/` for config
section comparisons, and `harness/` for evidence reports. `shared/` holds common
observation contracts. Tests and their original baselines live with each family.

The wrappers retired in 0.1.102 have these retained entries:

| Surface                                           | Current source and entry                                                                                             |
| ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Local board records and reopen                    | `crates/agent-collaboration/tests/e2e/board_records_e2e.rs` and `tests/board_e2e/`                                   |
| Local central replication, holders and forwarding | `crates/agent-collaboration/tests/e2e/board_central_e2e.rs`                                                          |
| Native central conformance                        | `crates/collaboration-central-tokio/tests/`, with explicit targets and `conformance` where required                  |
| Local Worker conformance                          | `composed_scope_workerd` and `session_scope_workerd` examples; use CI's pinned tools and scenario selections         |
| Two-machine Tokio, workerd or staging             | Unavailable until deliverable C; `node verification/run.ts` refuses with exit 2. Retained cases remain source for C. |

The Rust local process suites require the CI local-end-to-end job's prebuilt
entry points and owned runtime roots. The TypeScript runtime entries require
private declarations, qualified artifacts and an explicit execution commission.
Do not run them against production or infer authorization from this README.
Mapped cases determine coverage. Missing mappings remain open; a replacement
entry does not establish every historical obligation.
`targets/` runs worker-build bundles under the pinned Miniflare and workerd with real
SQLite-backed Durable Objects (`workerd-storage.ts`). Its cases are permanent
integration tests, run with `pnpm --dir verification run test:workerd` after building
the central Worker's test-support bundle:

```sh
worker-build crates/collaboration-central-cloudflare --dev --no-opt --out-dir build-test-support -- --features test-support,conformance
```

`workerd-central-storage.test.ts` covers the storage boundary on the Cloudflare host
(Verification O1): batch atomicity, constraint kinds with foreign keys enforced,
`SetFromQuery` alarms, in-transaction migrations, clock-minted id order,
`call_store`, the pause seam with a simulated crash, and every scenario of the
moved SQLx Durable Object driver (`/test-support/driver/`).

Release formula generation is
`node verification/tooling/update-homebrew-formula.ts --help`. The local tap
publisher named in `AGENTS.md` performs external publication and installation;
its unit tests substitute those external commands. Running tests authorizes no
tap push, tag, Homebrew operation or production process replacement.

The quota and compiled TUI apparatus under `scripts/verification/` is assigned
to PR #161 and remains outside this migration's ownership.
