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
| R1 | No E4 remains: shared test code is reached through a dependency on its owning E3. | U1, U3 | V1: the boundary check finds zero `#[path]` targets outside the declaring crate. Reference: the 38 listed above. |
| R2 | No production (normal or build) dependency reaches a test-support crate or a `test-support` feature. | U1, U3 | V2: cargo metadata for the release feature set shows no edge from a production target to an E3. Reference: Cargo resolver v2 feature unification, measured on the release selection, not the workspace test build. |
| R3 | Every package keeps at most one non-empty integration target and one non-empty e2e target, with a README naming its contract. | U1, U3 | V3: cargo metadata per package. Reference: the owner's test layout rule. |
| R4 | Every E1 keeps its identity and proof: the listed run and skipped case sets are identical before and after, per CI invocation. | U4 | V4: same-revision `cargo nextest list` pairs, as in the CI-to-30 design's O2. Reference: the pre-change listing. |
| R5 | Build CPU, link-inclusive elapsed time, target bytes and saved-cache bytes do not get worse on any measured scenario, and improve on at least one. | U2 | V5: an E5 pair per scenario. A regression on any scenario blocks the change it belongs to. |
| R6 | The rules for agents are written once, in AGENTS.md, and R1–R3 are enforced by checks in workspace-architecture-check. | U3 | V6: the rules text exists, and each check fails on a seeded violation (kept in the working tree only). |

### Failure cases

- **A move would widen a production API** (a private constructor becomes `pub`) only to reach it from a new support crate: the code stays with its owner under `cfg(test)`, or behind the owner's non-default `test-support` feature. A wider API is never the price of a move.
- **R5 regresses** on a scenario: that step is not merged. The measurement is reported to the owner.
- **A B2 file is involved:** that step waits until B2 lands.
