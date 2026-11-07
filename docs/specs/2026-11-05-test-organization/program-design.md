# Test organization: Program Design

This design realizes [specification.md](specification.md) in two phases. **Phase 1** moves only test-support code: each shared fixture gets one owning crate, or an owning crate's `test-support` feature, and consumers reach it through a dev-dependency. No test case moves, so every case keeps its binary id and name (R4). **Phase 2** is a measured pilot of one system-suite crate, behind owner question Q2; it is not part of this change.

## Structural choice

| Option | What it does | Gain | Cost | Verdict |
| --- | --- | --- | --- | --- |
| A · More test binaries per crate | split each crate's integration target by area | smaller binaries | each binary links the whole dependency graph: more link time, more target bytes; breaks the one-target rule | rejected |
| B · Support code gets owners (selected) | shared fixtures move into support crates or owner features; consumers depend on them | fixture source compiles once per support unit instead of once per consumer; dependency closures shrink to what is used; agents have one place for each fixture | new crate manifests; some fixture APIs become `pub` inside test-only crates | selected; gated by R5 measurement |
| C · System-suite crates now | move cross-crate journeys into new test crates | ownership by contract | loses same-package `CARGO_BIN_EXE_*` discovery; unproven CPU effect; changes test identities | deferred to a phase 2 pilot (Q2) |

Option B holds because the defect is shared source compilation and over-wide dependency closures, not the number of test targets. The Advisor's review (2026-11-05, recorded in the trace) reached the same verdict independently and supplied the corrections folded in below.

## Entity binding

| E | Owner | Home | Shape | Kind |
| --- | --- | --- | --- | --- |
| E1 Test case | nextest | existing test targets, unchanged in phase 1 | `<binary-id> <test-name>` | existing |
| E2 Test target | Cargo | existing `[[test]]` targets; `autotests = false` stays | Cargo metadata target | existing |
| E3 Test-support unit | the crate named in the table below | new crates under `crates/`, or an existing crate's `test-support` feature | an unpublished rlib (`publish = false`) | new / modified |
| E4 Cross-crate include | — | removed | — | removed |
| E5 Build measurement | the Lead | `tmp/` evidence plus the PR description | per scenario: toolchain, features, inventory hash, elapsed, user+sys CPU, target bytes, saved-cache bytes | derived |

## Support units (phase 1)

| Unit | Contents (from) | Consumers | Notes |
| --- | --- | --- | --- |
| `owned-process-test-support` (new crate) | `owned_cli_process`, `scenario_finalization` (from `codex-router-test-support`) | agent-collaboration, board-legacy-upgrade, codex-router-cli, codex-router-host | depends on no Router crate; `codex-router-test-support` re-exports nothing, and its callers switch to the new crate |
| `codex-router-test-support` (narrowed) | installed Codex, native app-server, router health, mock upstream, peer and Claude fixtures | as today, minus the process-only users | keeps the CLI/proxy/state closure, now only for consumers that use it |
| `collaboration-application-test-support` (new crate) | `board_fixture`, `native_authority`, `native_registry`, `native_mail_owner`, `accepted_snapshot`, `mail_cancel_expansions` (from `collaboration-service/tests/support/`) | collaboration-service tests, agent-collaboration, codex-router-host, collaboration-client, collaboration-mcp | depends on collaboration-service (its normal API, plus any hook behind collaboration-service's own `test-support` feature) and `board-central-test-support`. Builders that need service-private items stay in collaboration-service under `cfg(test)`. |
| `agent-collaboration-tui` feature `test-support` (existing feature) | `src/test_support/*` exported when the feature is on | agent-router-quota-tui, agent-session-tui, codex-router-cli (dev-dependency with the feature) | replaces 14 includes |
| `collaboration-runtime` feature `test-support` | `native_worker_faults` | collaboration-service | replaces 2 includes |
| `codex-acp-adapter` feature `test-support` | conversation operation recorder, native permission echo | collaboration-service | replaces 2 includes |
| `collaboration-central-tokio` feature `test-support` | `mail_relay_transport` | codex-router-host | replaces 1 include |
| `collaboration-central-conformance` | `workerd_process` as a library module of its own | collaboration-central-tokio examples | replaces 6 includes |
| `board-legacy-upgrade` feature `test-support` | `upload_support::actual_cli` | agent-collaboration e2e | replaces 1 include |
| `codex-router-quota-reset-test-harness` | the loopback provider *helpers* as a library module | codex-router-cli tests | replaces 1 include; the test case in that file stays (see below) |

**Named exceptions, left for the production design:** collaboration-central-tokio compiles collaboration-central-cloudflare's `object_location.rs` and `binding_attachment.rs` by path. The Cloudflare crate is a `cdylib` (`crates/collaboration-central-cloudflare/Cargo.toml:12`), so it can't be a dev-dependency. The real fix is a shared rlib for the Worker codec, which is a production-crate change, and `object_location.rs` is near B2's planned address-resolver work. These are the two exact R1 exceptions. They stay until the owner authorizes the production extraction.

**Case-bearing files are split, not moved whole.** A file that holds both reusable helpers and test cases moves only its helpers. Each case stays in its current target and module path, importing the helper from its new owner, so its binary id and name don't change (R4). Known instances:
- **`scenario_finalization`:** `scenario_finalization.rs:101-103` includes `scenario_finalization_tests.rs`, whose three cases sit at `:242`, `:270` and `:302` in the `codex-router-test-support` lib-test binary. The helper code moves to `owned-process-test-support`. The three cases stay in `codex-router-test-support`, which then dev-depends on the new crate, and they use its peer fixtures as before.
- **The loopback provider:** `loopback_provider_test.rs:348-349` defines `quota_refresh_response_plan_is_opt_in_and_retains_real_get_post_gates` inside the quota-reset e2e target (`tests/e2e/quota_reset/mod.rs:1-5`). The helpers move into the harness's library. The case stays in that file and module.

Before extraction, every file to be moved is checked for `#[test]`, `#[tokio::test]` and other test attributes, and every case found is recorded with its current binary id and name. That list is the R4 mapping.

**The fixture closure moves together.** `board_fixture.rs` depends on sibling support files: `board_context_backend`, `registered_project`, `board_poll_services`, `controlled_clock` and others. They move with it into `collaboration-application-test-support`, so no new include appears. The fixture's `pub(crate)` items become a suitable API inside that unpublished crate. This doesn't widen a production API: `ApplicationFixture` is already exported only under `cfg(test)` or `feature = "test-support"` (`collaboration-service/src/lib.rs:17-20`), and the constructors it calls are already public.

## Rules for agents (proposed AGENTS.md section)

1. **Where a test goes.**
   - A private implementation detail goes in the sibling `*_test.rs`.
   - One product's public API goes in that package's `tests/integration`.
   - A whole-process startup journey goes in `tests/e2e`.
   - A journey that spans several products stays with its primary product until a named system suite exists (phase 2).
2. **Shared test code.**
   - Used by one package: it lives in that package's tests.
   - Used by two or more packages: it lives in a named support crate (`*-test-support`, `publish = false`), or behind the owning crate's non-default `test-support` feature when it needs that crate's internals.
   - Never `#[path]` into another crate.
3. **Production never sees test support.**
   - A support crate or `test-support` feature appears only in `[dev-dependencies]`, or in another test-support crate.
   - A test never widens a production API (no `pub` added only for a test).
4. **Layout.**
   - Every package sets `autotests = false`.
   - It has at most one non-empty integration target and one non-empty e2e target, plus a README naming each target's contract.
   - Modules are named for their responsibility. The file-size limits apply.
5. **A new test crate needs a reason on record:**
   - a named contract and owner;
   - its dependency closure, compared with the alternative;
   - how it finds compiled artifacts (`CARGO_BIN_EXE_*` works only within a package);
   - a mapping that keeps every moved case;
   - a before/after measurement;
   - an Advisor check before it's built.
6. **Proof never changes in a move:** ignores, scheduling groups, priorities, deadlines, per-case process isolation and zero-match checks stay as they are.

## Enforcement (workspace-architecture-check)

- **R2, shipping-closure check, on the shipping platform:** the authoritative check runs natively on Apple Silicon. It runs in the release job, beside its existing guard, and on every main push in the existing Apple Silicon warm-up job (`ci.yml:512`, `macos-15`), so it doesn't wait for a release. A PR gets only the early warning below until it merges. A default Linux `cargo tree` would miss a macOS-conditioned dependency. As an early warning, CI Static may also run the same command with `--target aarch64-apple-darwin`. That view evaluates target dependencies for the shipping target, but build-script and proc-macro dependencies for the Linux build host, so it is labelled an early warning, never the shipping evidence. The command: `cargo tree --locked -p codex-router-cli -p agent-collaboration -p board-legacy-upgrade -e normal,build,features`, the release build's own selection (`release.yml:147`), and fail if a `*-test-support` crate or a `test-support` feature appears. This extends the release workflow's existing guard (`release.yml:184-193`), which uses the same method for `codex-router-secret-store`. It isn't a `crate_layers.rs` check, because that reader drops node features (`crate_layers.rs:115-119`), and Cargo metadata merges dev units, so neither can tell a dev-only activation from a shipping one.
- **R3, layout check:** in `crate_layers.rs`, read from Cargo metadata, which is right for manifest facts. For each package: `autotests = false`, at most one integration and one e2e target, and a README present. The 17-crate layer table also grows a rule: no production crate depends on a `*-test-support` crate.
- **R1, source-path check, only if the owner accepts it (Q1):** reads `#[path = "…"]` attributes, resolves each against its declaring file, and fails on any target outside the declaring crate, other than the two R1 exceptions. It also fails on a stale exception. If the owner declines it, R1 rests on rule 2 and on review.

## Proof seams

- **R1:** the source-path check, if accepted. Otherwise the include inventory recomputed at review.
- **R2:** the `cargo tree` shipping-closure check.
- **R3:** the layout check.
- **R4:** `cargo nextest list` inventories from the pinned pre-extraction and post-extraction trees, for the six affected CI selections.
- **R5:** E5 pairs, built on the same CI machine type with identical toolchain, features and inventory.
- **R6:** the AGENTS.md text, plus a seeded violation per check, kept in the working tree only.

## Trace

| U | R | Owner | Seam |
| --- | --- | --- | --- |
| U1, U3 | R1 | each support unit | source-path check (if Q1 accepted), else review inventory |
| U1, U3 | R2 | native Apple Silicon release job, beside its existing guard (plus a labelled `--target` early warning in Static) | `cargo tree -e normal,build,features` on the release selection |
| U1, U3 | R3 | `crate_layers.rs` (layout) | package metadata |
| U4 | R4 | nextest config, unchanged | list pairs |
| U2 | R5 | the Lead | E5 measurements |
| U3 | R6 | AGENTS.md and the checks | text plus seeded red |

## Measured context (not a selected change)

The first timing report of the whole test build, on 16 vCPU (run 42045129233; `tmp/design-workflows/2026-11-05-test-organization/`), took 251 s wall:
- **Test units:** 98, with 962 s of summed unit time.
- **Dependency units:** 892, with 956 s.
- **Longest units:** collaboration-central-tokio's integration test binary at 122 s, collaboration-service's lib tests at 58 s and its integration tests at 56 s.
- **Two caveats:** that step's selection included e2e targets the nextest build excludes, and `turso_core` 0.8.1 has four timing entries with matching features, two of them library builds (58 s and 60 s). So it isn't yet a clean "before" sample for R5. Its value is pointing at the central-tokio integration binary as the one place where Option A (splitting a target) could pay off. That remains a phase-2 candidate, measured like Q2.
