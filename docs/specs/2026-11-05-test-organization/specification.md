# Test organization: Specification

The workspace's test code is organized so that every piece of shared test code has one owning crate, reached through a dependency, never through a source path into another crate. Production code never depends on test-support code. Every test case keeps its proof. The needs are in [requirements.md](requirements.md).

## Current behavior

- **38 cross-crate `#[path]` includes** compile one crate's file inside another. Measured on main `e34ebef87` by resolving each `#[path]` against its declaring file:

| Source owner | Included into | Files |
| --- | --- | --- |
| collaboration-service `tests/support/` | agent-collaboration (integration), codex-router-host (lib tests, integration), collaboration-client (integration), collaboration-mcp (`src/lib.rs`) | `board_fixture.rs` (619 lines, three consumers), `native_authority.rs`, `native_registry.rs`, `native_mail_owner.rs`, `accepted_snapshot.rs`, `mail_cancel_expansions.rs` |
| agent-collaboration-tui `src/test_support/` | agent-router-quota-tui, agent-session-tui, codex-router-cli | eight frame and event modules |
| collaboration-central-cloudflare `src/` (production files) | collaboration-central-tokio (integration) | `binding_attachment.rs`, `object_location.rs` |
| collaboration-central-conformance `examples/support/` | collaboration-central-tokio (examples) | `workerd_process.rs` (six includes) |
| collaboration-central-tokio `tests/support/` | codex-router-host (lib tests) | `mail_relay_transport.rs` |
| collaboration-runtime `tests/support/` | collaboration-service (`src/lib.rs`, integration) | `native_worker_faults.rs` |
| codex-acp-adapter `tests/support/` | collaboration-service (integration) | two recorder and echo fixtures |
| board-legacy-upgrade `tests/upload_support/` | agent-collaboration (e2e) | `actual_cli.rs` |
| codex-router-quota-reset-test-harness `tests/e2e/` | codex-router-cli (`src/`) | `loopback_provider_test.rs` |

- **`codex-router-test-support` mixes two closures.** It always depends on codex-router-cli (with `keychain-test-support`), codex-router-proxy and codex-router-state (`crates/codex-router-test-support/Cargo.toml:12-20`). Its process-ownership modules (`owned_cli_process`, `scenario_finalization`) need none of them, yet board-legacy-upgrade, which uses only those two, pays the full closure.
- **Layer policy is partial.** `crates/workspace-architecture-check/tests/integration/crate_layers.rs:9-95` checks only the 17 crates in its table; test-support crates and central-core are outside it.

## Entities

| ID | Term | Identity | Relationships | Invariants | States |
| --- | --- | --- | --- | --- | --- |
| E1 | Test case | binary id + test name as nextest lists it | belongs to one test target | its inputs, assertions, deadlines, group and priority are its proof | listed → run or skipped |
| E2 | Test target | package + target kind (lib, integration, e2e) | 1..n cases; one package | at most one integration and one e2e target per package | — |
| E3 | Test-support unit | the crate (or crate feature) that owns a piece of shared test code | used by 1..n test targets | exactly one owner; consumers reach it by dependency | — |
| E4 | Cross-crate source include | a `#[path]` whose resolved target lies outside the declaring crate | links a consumer file to a foreign file | — | present → removed |
| E5 | Build measurement | toolchain + features + case inventory + scenario (cold, dependency-cached, test-only edit, production edit) | compares a before and an after | same toolchain, features and inventory on both sides | — |

## Obligations

| ID | Obligation | Traces to | Proof obligation |
| --- | --- | --- | --- |
| R1 | No E4 remains except two named exceptions: collaboration-central-tokio's `tests/integration/binding_attachment.rs` → collaboration-central-cloudflare `src/binding_attachment.rs`, and `tests/integration/object_identity.rs` → collaboration-central-cloudflare `src/object_location.rs`. They exist because the Cloudflare crate is a `cdylib` and can't be a dependency. They expire when the owner authorizes the production extraction of the Worker codec. All other shared test code is reached through a dependency on its owning E3. | U1, U3, U5 | V1: exactly those two edges remain. A third cross-crate edge fails, and so does an exception whose file or target no longer exists. Reference: the 38 edges listed above, recomputed at c625da0b6 by the independent review. |
| R2 | No test-support crate and no `test-support` feature is in the shipping build's normal or build dependency closure. A dev-dependency activation used only in tests is allowed. | U1, U3 | V2: on the shipping platform, Apple Silicon (`aarch64-apple-darwin`, the native release job, `release.yml:35-39`), `cargo tree --locked -p codex-router-cli -p agent-collaboration -p board-legacy-upgrade -e normal,build,features`, the release build's selection (`release.yml:147`), shows no test-support crate and no `test-support` feature. This reuses the release's existing guard method (`release.yml:184-193`), which excludes dev units before resolving features. Cargo metadata is used only for manifest and topology facts, because its resolver merges dev units (Cargo `src/ops/cargo_metadata.rs:148-168`). Reference: the workspace resolver is 3. |
| R3 | Every package keeps at most one non-empty integration target and one non-empty e2e target, with a README naming its contract. | U1, U3 | V3: cargo metadata per package. Reference: the owner's test layout rule. |
| R4 | Every E1 keeps its identity and proof: for every affected CI selection, the run and skipped case sets recorded on the pre-extraction tree equal those on the post-extraction tree. | U4 | V4: two pinned source trees (before and after extraction), with the same toolchain and the same selection arguments, each producing a `cargo nextest list` inventory. Selections (six): the workspace run (`ci.yml:310`; its build at `:295`), the quota-reset run (`:304`; its build at `:299`), the isolated port-cutover run (`:316`), and the three separate e2e runs (`:389-391`). Equality is checked on the observed sets, never inferred from the flags. Reference: the recorded pre-extraction inventory. |
| R5 | Build CPU, link-inclusive elapsed time, target bytes and saved-cache bytes are not worse on any measured scenario, and are better on at least one, under the decision rule in the Verification Design. | U2 | V5: E5 samples per scenario, read by that rule. A regression on any scenario blocks the change it belongs to. |
| R6 | The rules for agents are written once, in AGENTS.md. R2 and R3 are enforced by checks in workspace-architecture-check. R1's source-path check is enforced only if the owner accepts it (Q1); otherwise R1 rests on the rule and on review. | U3 | V6: the rules text exists, and each enforced check fails on a seeded violation, kept in the working tree only. |

### Failure cases

- **A move would widen a production API** (a private constructor becomes `pub`) only to reach it from a new support crate: the code stays with its owner under `cfg(test)`, or behind the owner's non-default `test-support` feature. A wider API is never the price of a move.
- **R5 regresses** on a scenario: that step is not merged. The measurement is reported to the owner.
- **A B2 file is involved:** that step waits until B2 lands.
