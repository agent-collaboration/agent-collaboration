# Plan: CI to 30 minutes (scheduling slice), on D1's code

```text
plan path: docs/specs/2026-11-04-ci-30-minutes/plans/2026-11-05-ci-30-minutes-on-d1.md
originating planner: plan-update
supersedes: docs/specs/2026-11-04-ci-30-minutes/plans/2026-11-04-ci-30-minutes.md
planning result: ready
governing planning basis:
  kind: reviewed-four-document-design
  Requirements:        docs/specs/2026-11-04-ci-30-minutes/requirements.md
  Specification:       docs/specs/2026-11-04-ci-30-minutes/specification.md (R7 and its failure case corrected for the D1 base)
  Program Design:      docs/specs/2026-11-04-ci-30-minutes/program-design.md (S2 anchors and R7 seam corrected)
  Verification Design: docs/specs/2026-11-04-ci-30-minutes/verification-design.md (U1 and U6 rows, O4 corrected)
  review: four-document-design, an independent Sol xhigh Reviewer, ready at 0f08b8b89; the base-change corrections go to the same Reviewer to verify the corrected anchors
  current applicability anchors: D1 feat/central-storage-reset-d1-router 71e466dbf (frozen except CI or merge-readiness fixes, per the D1 Lead 2026-11-05)
delivery context:
  requested terminal: pr-ready-unmerged
  delivery grouping: single:scheduling
  PR topology: one-pr, base feat/central-storage-reset-d1-router until D1 lands, then main
  repositories: agent-collaboration/agent-collaboration
```

## Changed from the 2026-11-04 plan

The owner asked on 2026-11-05 to build this on top of D1 instead of main. What that changes:

- **Base.** Branch `ci/scheduling-30-min` starts from D1 `71e466dbf` plus the design commits (head `6f3da79df` when the helpers started). Invalidates: nothing already built (no slice had started).
- **S2 anchors.** At D1 `71e466dbf` the test file is unchanged from main: `crates/codex-router-host/src/external_provider_runtime/tests/admission_lifecycle_test.rs`, the held-initialize pattern from line 401, the oversized test at line 444. The old plan's "D1 `admission_lifecycle_tests.rs:374-411`" anchor and the hand-off to D1's Lead are gone.
- **S1 and S3 baseline.** The baseline log is D1's green run 41994005347 (test job 127133117707), not main's 41826050980. Main's log is kept as a comparison.
- **S4.** "Merge-tree against D1" becomes "merge D1's head forward" while D1 is open, and "merge main forward" after D1 lands. The repository allows only squash merges, so D1's commits won't appear on main as they are. Merging main (with D1's squash commit) into the branch keeps the PR's diff to this change only. No rebase.
- **S5 and the PR.** The PR targets D1's branch while D1 is open and is retargeted to main after D1 lands. U1 times that PR run and the first main run after both have landed (Verification Design U1 row).
- **Outside this slice, now known.** On D1's green run, Static validation took 50 min (ratchet 43.8) and e2e 41 min. This slice cannot bring those under 30. R1 will be missed on those jobs. That's Q1's trigger, already designed: report the measured profile, and the PR remains a valid improvement. The widened design for Static and e2e is separate work, and the owner has the ratchet question.

## Goal and scope

Unchanged from the 2026-11-04 plan. Make the workspace nextest invocation start the right tests first and run 8 threads, without changing which tests run or what they prove. Then measure the PR's CI run.
- **In scope:** the profile tool; priority overrides and `test-threads`; the causal fix for `oversized_initialize_frame_is_rejected_and_reaps_owned_process`; a causal fix for any other test that fails only under the new scheduling.
- **Non-goals:** the remaining run-alone causal conversions; ratchet speed; e2e; crate splits; options B and C.
- **No version bump:** CI and test-only changes.

## Who does it, and where

- **Implementer:** 🐒 Sol high on the build machine, in the assigned repository worktree.
- **Verifier:** 🔎 Luna high on the build machine, in the same assigned worktree, building only after the Implementer hands back.
- **Review:** an independent Sol xhigh Reviewer reviews the PR.
- **Commits** use `--no-gpg-sign`. No git stash. Files stay under 1,000 lines.

## Slices

### S1 · Profile tool

Reads, writes, pre-change signal and oracle carry forward from the 2026-11-04 plan. The profile is run on D1 run 41994005347 and on main run 41826050980.

- **Behavior (corrected 2026-11-05 after the Implementer's S1 stop):** the downloaded log of 41994005347 has `UNKNOWN STEP` in every line, so step names can't separate invocations. The tool groups result lines by nextest's run total, the `<total>` in `(<n>/<total>)`, and checks each group against the `Summary` with the same `N tests run`. That log has three: 60, 6806 and 1. The shapes are in Program Design "Entity binding". The #213-style fixture still models two concurrent invocations, now told apart by their totals, plus one case with two equal totals that must return `ambiguousInvocations`.
- **Stop:** a log whose result lines carry no `(<n>/<total>)` counter returns to the Lead.

### S2 · Causal fix for the oversized-frame test

As in the 2026-11-04 plan, with the anchors above. Its proof is the O5 pair on the build machine, run by the Verifier: the pre-fix test at the branch base and the fixed test, each with the fixture's start delayed beyond 2 s. Also `cargo nextest run -p codex-router-host -E 'test(admission_lifecycle)'`. Its stop conditions carry forward. The "port only the watcher helper" stop no longer applies, because the pattern is in the same file.

### S3 · Priorities and 8 threads

As in the 2026-11-04 plan, with these changes:
- **The 50 band:** the list of tests measured at ≥ 30 s comes from S1's profile of D1 run 41994005347.
- **O4 selector half:** listed on the branch head only, which is D1's code.
- **O2 list pair:** unchanged, same revision.

### S4 · Merge D1 forward (integration)

- **Action, while D1 is open:** before the PR and whenever D1's head moves:
  1. `git merge-tree --write-tree HEAD origin/feat/central-storage-reset-d1-router`;
  2. if it's clean, `git merge` (no rebase);
  3. re-run the per-selector listing on the merged head.
- **Action, after D1 lands:** the same steps with `origin/main` in place of the D1 branch, then retarget the PR to main.
- **Proof:** merge-tree output and the listing, recorded in the PR description.
- **Stop:** a conflict that can't be resolved by moving appended blocks returns to the Lead.

### S5 · PR run and measurement (integration gate)

- **The PR:** opened from `ci/scheduling-30-min` with base `feat/central-storage-reset-d1-router`, then checked with `gh pr checks <n> --watch --interval 240`.
- **O1:**
  - creation time to the last required check, plus each required job's duration;
  - the job count (≤ 4);
  - the ratchet step present and successful.
- **O2:**
  - S1's tool run on the test-job log;
  - both inventories match their Summaries;
  - the workspace profile is reported.
- **U3/R3 Summary half:** unchanged.
- **Result handling:** as in the 2026-11-04 plan.
  - The expected first outcome is the test job improved, with R1 missed on Static and e2e, which are outside this slice. That's reported to the owner with the per-job times.
  - R1 is re-checked on the first main run after both D1 and this change land.

## Dependency edges

- `requires S1 -> S3`
- `requires S2 -> S5`
- `requires S3 -> S4 -> S5`
- `parallel S1 || S2`

## Obligation-to-proof map

| Verification Design row | Slice | Command or journey | When |
| --- | --- | --- | --- |
| U1 / R1 | S5, and the first main run after both land | `gh api …/actions/runs/<id>` and `/jobs` | after PR CI; after both land |
| U2 / R2 | S5 | the same `/jobs` count | after PR CI |
| U3 / R3 | S3 (list pair), S5 (Summary match) | `cargo nextest list --profile ci …` ×2 per invocation on the build machine; the S1 tool on the CI log | S3 before the PR; S5 after CI |
| U3 / R4 | S2 and every S5 fix | PR diff review against the pre-change files | at review |
| U4 / R5 | S2 (and S5 fixes) | delayed-start pre-fix and fixed runs on the build machine, plus the 8-thread PR run | before the PR; after CI |
| U5 / R6 | S5 | `static` job steps from `/jobs` | after PR CI |
| U6 / R7 | S3 (selectors), S4 (merge-forward) | `git merge-tree`, merge, per-selector `nextest list` on the merged head | before the PR; each time D1's head moves; after D1 lands |

## Integration gates

- **Gate A (before the PR):**
  - S1's tests pass;
  - the S2 O5 pair is observed;
  - S3's list pair and selectors match;
  - S4 merges D1's head clean.
- **Gate B (the PR):**
  - CI green, and O1 and O2 recorded;
  - independent review with 0 open findings;
  - merge after D1, under the owner's release authority as usual.

## Risks and false greens

These carry forward from the 2026-11-04 plan, plus two more:
- **The PR run's build time overstates the post-merge cost.** PR runs reuse main's cache, which only partly matches D1's `Cargo.lock`. This is recorded beside each build figure.
- **D1 is still moving.** It's frozen except for CI fixes, but a late D1 commit could rename a selected test. Guarded by S4 re-running whenever D1's head moves.

## Stop and replan conditions

As in the 2026-11-04 plan. Also: D1 is abandoned or re-cut. In that case the base returns to main through a new plan update.
