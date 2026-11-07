# Plan: Test organization, phase 1

```text
plan path: docs/specs/2026-11-05-test-organization/plans/2026-11-05-test-organization-phase-1.md
originating planner: plan-implementation
supersedes: none
planning result: ready
governing planning basis:
  kind: reviewed-four-document-design
  Requirements:        docs/specs/2026-11-05-test-organization/requirements.md
  Specification:       docs/specs/2026-11-05-test-organization/specification.md
  Program Design:      docs/specs/2026-11-05-test-organization/program-design.md
  Verification Design: docs/specs/2026-11-05-test-organization/verification-design.md
  review: four-document-design, independent Sol xhigh Reviewer; findings F1–F5 at c625da0b6, corrected at 7c9e75365 and e4825ae32, final verdict ready
  Q1: the source-path check is kept, routed by the owner's chief of staff on 2026-11-05 on the Lead's recommendation; not recorded as an owner decision
  current applicability anchors: main 9ad3c0db0 (PR #217 merged); 38 cross-crate includes recounted at f793385b4
delivery context:
  requested terminal: pr-ready-unmerged
  delivery grouping: single:test-support-ownership
  PR topology: one-pr
  repositories: agent-collaboration/agent-collaboration
```

Planned at branch `design/test-organization` `f793385b4`, which is main `9ad3c0db0` plus the design.

## Goal and scope

Give every shared test fixture one owning crate or owning-crate feature. Remove 36 of the 38 cross-crate `#[path]` includes. Keep production free of test support, and write the rules for agents, with checks that enforce them. No test case changes identity, and the change merges only if the build-cost gate (R5) passes.

- **In scope:** the support units in Program Design "Support units (phase 1)"; the case-bearing splits; the R1 source-path check (Q1 kept); the R2 shipping-closure check; the R3 layout check; the AGENTS.md section; and the before/after measurements.
- **Non-goals:**
  - moving test cases, or system-suite crates (phase 2);
  - splitting central-tokio's integration target (a phase-2 candidate);
  - the two Cloudflare codec includes (named exceptions);
  - any production-crate split;
  - any file B2 has in flight in collaboration-central-core.
- **No version bump:** tests, test-only crates, CI checks and docs change; nothing ships to users.

## Who does it, and where

- **Implementer:** the existing Sol high 🐒 Implementer, on the build machine, in a new worktree on branch `tests/support-ownership`, created from `design/test-organization`.
- **Verifier:** the existing Luna 🔎 Verifier, paired with it.
- **Review:** an independent Sol xhigh Reviewer reviews the PR.
- **Working rules:** commits use `--no-gpg-sign`; no git stash; files stay under 1,000 lines; TypeScript or Rust only.

## Obligation ledger

| Obligation | Owner and path today | Change | Proof row |
| --- | --- | --- | --- |
| R1, no cross-crate include except two | 38 includes (Specification "Current behavior") | S2–S4 remove 36; S5 adds the check | U1/U3 R1 · O1 |
| R2, no test support in the shipping closure | `release.yml:184-193` guards only the secret store | S5 extends the guard and adds the warm-up job check and the Static early warning | U1/U3 R2 · O2 |
| R3, layout | owner rule, unenforced | S5 layout check in `crate_layers.rs` | U1/U3 R3 · O3 |
| R4, case identities kept | — | S1 records P; S7 compares Q | U4 R4 · O4 |
| R5, build cost not worse, better somewhere | — | S1 before samples; S7 after samples | U2 R5 · O5 |
| R6, rules and enforced checks | AGENTS.md has none | S6 rules; S5 seeded reds | U3 R6 · O6 |

## Slices

### S1 · Baseline: P inventories, include inventory, case map, before samples (proof-only)

- **Reads:** Specification R1, R4 and R5; Verification Design O1, O4 and O5, and "Decision rule for R5"; Program Design "Case-bearing files are split".
- **Writes:** none in the product tree. Evidence goes in the worktree's `tmp/test-organization/`. The measurement workflow lives on a separate, never-merged experiment branch, `ci/test-org-measure`, which adds one job. That job runs the four O5 scenarios in sequence on the CI machine type and uploads `/usr/bin/time -v` output, `du -sb target/debug` and the saved-cache size.
- **Produces:**
  - P's `cargo nextest list --message-format json` inventory for the six selections: `ci.yml:310` (build `:295`), `:304` (build `:299`), `:316` and `:389-391`;
  - the 38-edge include inventory;
  - a scan of every file to be moved for `#[test]`, `#[tokio::test]` and other test attributes, recording each case's binary id and name: the R4 mapping;
  - two before samples per scenario.
- **Stop:** if a case-bearing file appears beyond the two named in Program Design, record it in the mapping and split it the same way. If splitting would need a production API change, return to the Lead.

### S2 · `owned-process-test-support` (vertical)

- **Reads:** Program Design "Support units" rows 1–2, and "Case-bearing files"; Fleet access constraints (none apply).
- **Writes:**
  - a new `crates/owned-process-test-support/`, unpublished, holding the `owned_cli_process` and `scenario_finalization` helpers;
  - `codex-router-test-support` narrowed to keep its three `scenario_finalization` cases (`:242`, `:270`, `:302`) in place, using the new crate as a dev-dependency;
  - consumers switched: agent-collaboration, board-legacy-upgrade, codex-router-cli and codex-router-host.
- **Pre-change signal:** board-legacy-upgrade's dependency closure contains codex-router-cli, codex-router-proxy and codex-router-state through `codex-router-test-support`. After the change it must not. Oracle: `cargo tree -p board-legacy-upgrade -e normal,dev`.
- **Proof:**
  - `cargo nextest run` for each switched package and for `codex-router-test-support` (whose three cases keep their identity);
  - `cargo tree`, before and after, for board-legacy-upgrade.
- **Existing tests:** all keep. No case moves.

### S3 · `collaboration-application-test-support` (vertical)

- **Reads:** Program Design "Support units" row 3, and "The fixture closure moves together".
- **Writes:**
  - a new unpublished crate, holding `board_fixture`, `native_authority`, `native_registry`, `native_mail_owner`, `accepted_snapshot` and `mail_cancel_expansions`, plus their sibling helpers (`board_context_backend`, `registered_project`, `board_poll_services`, `controlled_clock` and any others the closure needs);
  - consumers switched from `#[path]` to the dependency: collaboration-service tests, agent-collaboration, codex-router-host (lib tests and integration), collaboration-client and collaboration-mcp.
- **Pre-change signal:** 9 includes into `collaboration-service/tests/support/`. After the change, none remain.
- **Proof:** `cargo nextest run` for every consumer package.
- **Existing tests:** keep.
- **Stop:** if any helper needs a collaboration-service private item, it stays in collaboration-service under `cfg(test)`. If that is impossible without widening a production API, return to the Lead.

### S4 · Owner features and libraries (vertical)

- **Reads:** Program Design "Support units" rows 4–10.
- **Writes:**
  - `test-support` feature exports: agent-collaboration-tui `src/test_support/*`, collaboration-runtime `native_worker_faults`, codex-acp-adapter's recorder and echo, collaboration-central-tokio `mail_relay_transport`, board-legacy-upgrade `upload_support::actual_cli`;
  - library modules: collaboration-central-conformance `workerd_process`, and the quota-reset harness loopback helpers. That harness's case `quota_refresh_response_plan_is_opt_in_and_retains_real_get_post_gates` stays in its file and module.
  - Consumers switch to dev-dependencies that enable the feature.
- **Pre-change signal:** 27 includes in these groups (14 + 2 + 2 + 1 + 1 + 6 + 1). After the change, none remain.
- **Proof:** `cargo nextest run` for each owner and consumer package, plus the central-tokio examples build.
- **Existing tests:** keep.

### S5 · Checks (vertical)

- **Reads:** Program Design "Enforcement"; Specification R1–R3; Verification Design O1–O3 and O6.
- **Writes:**
  - **R3:** a layout check in `crates/workspace-architecture-check/tests/integration/crate_layers.rs`, read from Cargo metadata: `autotests = false`, at most one integration and one e2e target, and a README. It also adds the rule that no production crate depends on a `*-test-support` crate, and fixes the stale README link (`architecture/README.md:42`).
  - **R1:** the source-path check, in the same crate. It resolves each `#[path]` against its declaring file, allows exactly the two Cloudflare exceptions, and fails on a third edge or a stale exception.
  - **R2:** the shipping-closure guard. In `release.yml`, it extends the existing guard at `:184-193`. In `ci.yml`, it is added to the macOS warm-up job at `:512`, which runs on main pushes, and the Static job gets the `--target aarch64-apple-darwin` early warning, labelled as such.
- **Pre-change signal:** on the S1 tree, the R1 check would report 38 edges; the R3 check would report every package missing `autotests = false` or a README, if any.
- **Proof (O6):**
  - one planted violation per check, in the working tree only, never committed: each check fails, then passes once it's removed;
  - `cargo nextest run -p workspace-architecture-check`.
- **Existing tests:** keep the layer-policy tests and extend them.

### S6 · Rules for agents (docs)

- **Reads:** Program Design "Rules for agents".
- **Writes:** one AGENTS.md section with the six rules, worded as the Program Design states them. The rules for this PR only, nothing else in AGENTS.md (owner rule: write only the rule asked for).
- **Proof:** O6 reads the text, and review.

### S7 · Q inventories, after samples, PR (integration gate)

- **Reads:** Verification Design R4 and R5 rows, and O4 and O5.
- **Action:**
  1. Q inventories for the six selections at the PR head, compared with S1's P.
  2. Two after samples per scenario on the experiment workflow, rebased onto the PR head by merge.
  3. The R5 decision rule, applied.
  4. Open one PR from `tests/support-ownership`, then `gh pr checks <n> --watch --interval 240`.
- **Result handling:**
  - **R4 unequal:** stop, and find the dropped or renamed case.
  - **R5 worse on any measure:** the PR doesn't merge. Report to the Lead with the numbers.
  - **R5 neither worse nor better:** report to the Lead. The owner judges the hygiene case.

## Dependency edges

- `requires S1 -> S2, S3, S4`: P and the case map must exist first.
- `serial S2 <-> S3 <-> S4`: they share `Cargo.lock` and several consumer manifests.
- `requires S2, S3, S4 -> S5`: the R1 check passes only once the 36 includes are gone.
- `requires S5 -> S6`: the rules name the checks.
- `requires S6 -> S7`.

## Obligation-to-proof map

| Verification Design row | Slice | Command or journey | When |
| --- | --- | --- | --- |
| U1/U3 R1 (O1) | S5 | `cargo nextest run -p workspace-architecture-check`; seeded third edge and stale exception | after S4 |
| U1/U3 R2 (O2) | S5 | `cargo tree --locked -p codex-router-cli -p agent-collaboration -p board-legacy-upgrade -e normal,build,features`, natively on Apple Silicon (the warm-up job on the first main push after merge; the release job on the next release); Static `--target aarch64-apple-darwin` early warning on the PR | PR CI; main after merge |
| U1/U3 R3 (O3) | S5 | layout check in `crate_layers.rs` | after S4 |
| U4 R4 (O4) | S1 (P), S7 (Q) | `cargo nextest list --message-format json`, six selections | S1 first; S7 before the PR |
| U2 R5 (O5) | S1 (before), S7 (after) | measurement job on `ci/test-org-measure`, two samples × four scenarios each side | S1; S7 |
| U3 R6 (O6) | S5, S6 | seeded reds; AGENTS.md text | S5; S6 |

## Integration gates

- **Gate A, before the PR:**
  - S2–S4 package runs green;
  - the S5 checks green, and each seeded red observed;
  - R4 Q equals P;
  - R5 decided.
- **Gate B, the PR:**
  - CI green;
  - independent review with 0 open findings;
  - merge under the owner's release authority.
- **After merge:** the native Apple Silicon R2 check passes on the first main push.

## Risks and false greens

- **A moved helper could drag its consumers' closure wider.** S2's `cargo tree` before/after guards board-legacy-upgrade. R5's cache-size measure guards overall.
- **R4 could pass on flags rather than observed sets.** O4 compares the observed P and Q sets, not the selection arguments.
- **The R2 early warning differs from the shipping closure.** It is labelled; the native check is the evidence.
- **A check that cannot fail.** Each has a seeded red (O6).
- **R5 noise.** The fixed rule in the Verification Design decides it.

## Stop and replan conditions

- A move needs a production API widened: return to the Lead.
- A file in B2's list is touched: stop.
- R4 unequal, or R5 worse: the PR is held. Report to the Lead.
- An include whose owner can't take a feature or library (as with the Cloudflare `cdylib`): return to the Lead before adding a third exception.
