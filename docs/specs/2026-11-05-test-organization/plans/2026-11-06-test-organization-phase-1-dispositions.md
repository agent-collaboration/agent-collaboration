# Test organization phase 1: Lead dispositions for S3 and S4

originating planner: plan-update
supersedes: plans/2026-11-05-test-organization-phase-1.md (for the three units below only; every other slice, edge, gate and proof carries forward unchanged)
governing basis: the reviewed four-document design (ready at e4825ae32, O5 clarified at ad7c4b38c), with the program-design corrections below
requested terminal: pr-ready-unmerged, one PR (unchanged)

## Changed from

Implementation found three places where placement alone could not satisfy the design. Each disposition below keeps the design's outcomes: one owner per shared fixture, no foreign `#[path]` beyond the two Cloudflare exceptions, production never sees test support, and the same test identities in all six CI selections. None of them widens a production API, touches a B2 file, weakens an assertion or relaxes the module-cycle ratchet.

### D1 · S3 NativeMailOwner splits by responsibility

- **Found:** `native_mail_owner.rs` does two jobs: application assembly (it builds a board application) and runtime-task ownership (it owns the Service's background tasks).
  - Placed in the external `collaboration-application-test-support` crate, it fails the Service's own unit tests: a second copy of the Service's types, and private hooks (59 compile errors).
  - Placed at the Service root behind `test-support`, it passes behavior, but adds two incoming edges to a module cycle, so the ratchet fails.
  - Nested under the runtime module, it grows an existing 5-module cycle to 7.
- **Disposition:** split the helper by its two jobs, as the Implementer recommended.
  - The pure runtime-task owner stays in `collaboration-service` behind its existing `test-support` feature (`cfg(any(test, feature = "test-support"))`). It sits in the module that already owns those tasks and imports nothing from application assembly.
  - Application assembly moves to `collaboration-application-test-support`. It builds through the Service's existing builder and validation, plus the existing Service-private factory exposed only under `test-support`.
- **Proof:**
  - The ratchet passes with the baseline unchanged: no new edge into any cycle, and no cycle grows.
  - The S3 package and Agent E2E receipts are rerun on the split.
  - The nine Service foreign includes stay removed.
- **Stop if:** the split needs a new production `pub` item outside `test-support`, or any ratchet baseline change. Bring the evidence back to the Lead.

### D2 · S4 legacy fixture gets its own unpublished support crate

- **Found:** the planned `board-legacy-upgrade` `test-support` feature library needs a normal (optional) dependency on `owned-process-test-support`. That puts a test-support crate in a production crate's normal dependency set, which R2 and S5 forbid.
- **Disposition:** a new crate `legacy-upgrade-test-support`, `publish = false`, reached only through dev-dependencies. It owns the `actual_cli` fixture. `board-legacy-upgrade` loses its foreign include and gains no test-support feature.
- **Reason on record (agent rule 5):**
  - Contract and owner: the legacy importer's real-CLI fixture, owned by the legacy-upgrade area.
  - Dependency closure: `owned-process-test-support` plus what the fixture already uses.
  - Binaries: found the same way as today.
  - Cases: none move.
  - Measured in the after-samples.
  - It's forced by R2: no owner feature is possible without a production-to-test-support edge. The independent PR review checks this in place of a separate Advisor pass.
- **Proof:** R2's shipping closure stays clean, the S5 adjacency check passes without exceptions, and the legacy E2E and package receipts pass.

### D3 · S4 quota loopback: the CLI's duplicate case identity is removed, with redundancy proof

- **Found:** `loopback_provider_test.rs` is compiled three times:
  - twice in the quota harness E2E binary (the `quota_reset` and `quota_storyboard` modules);
  - once in `codex-router-cli`, under its opt-in effectful-fixture module (`quota/quota_effectful_fixture/mod.rs:7`).
  - The CLI copy adds the identity `quota::…::loopback_provider::quota_refresh_response_plan_is_opt_in_and_retains_real_get_post_gates`. It's observed only in an all-features run, and none of the six CI selections runs it.
- **Disposition:** the CLI gets `HeldLoopbackProvider` from the quota-reset harness library, as planned, and stops compiling the case file. The third identity is removed as a duplicate.
- **Redundancy proof, recorded in the PR body:**
  - The test body exercises only `HeldLoopbackProvider` itself, a fixture self-test, not CLI code. The CLI copy runs the identical body against the identical code.
  - The case keeps both quota E2E identities in CI.
  - Q = P holds for all six selections, because the removed identity was never in P.
  - The PR body lists the removed identity by name, with this reason.
- **Not allowed:** a wrapper, macro or callback that recreates the identity, or a third `#[path]` exception.

## Unchanged

S5 → S6 → S7 order and gates; the R5 decision rule; the after-samples on GitHub 4-vCPU runners (`ci/test-org-measure` stays never-merged); one PR; the independent Sol xhigh implementation review before merge.
