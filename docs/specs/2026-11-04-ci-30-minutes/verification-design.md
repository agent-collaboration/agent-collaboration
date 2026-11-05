# CI to 30 minutes: Verification Design

Every requirement is proven on the real target: a GitHub-hosted CI run of the PR that carries the change, read through GitHub's own records and nextest's own output, plus same-revision listings and controlled test runs on the build machine. Nothing here has been observed yet; this is the plan for what will be.

## Proven by (owner view)

| Need | Proven by | Stays unverified |
| --- | --- | --- |
| U1 · CI in 30 min | Two CI runs are timed: the PR's own run (on D1's code) and the first main run after both D1 and this change have landed. Each is measured from GitHub's creation time to its last required check finishing, and compared with 30 minutes. Each invocation's profile shows where any time over 30 went. | runs under heavy org-wide queueing; D1's own test mix until D1's first PR run |
| U2 · no extra machines | The PR run's job list is counted. It must not exceed 4. | — |
| U3 · same tests, same proof | At one source revision, nextest lists the tests it would run and skip, with the new config and with the old one, for each invocation. The two lists must be identical. On Linux CI, the PR run's tests and skip counts must equal those of main's last pre-change run (42023755753 at 614c03b75); the build machine's lists are not compared with Linux counts, because platform-gated tests differ. Each changed test's diff shows its fixtures, assertions and deadlines kept. | — |
| U4 · causal waits | Each changed test is run with its fixture's start deliberately delayed, both before the fix (it fails) and after (it passes). The CI run with threads following the CPU count also has no failures. | load-sensitive tests no CI run has exposed yet |
| U5 · ratchet stays | The Static validation job still runs the ratchet step, and it passes. | — |
| U6 · works with D1 | The change is built on D1. Each new D1 head is merged in with no conflicts, and each test a priority rule names is listed separately on the merged code and found. The same is done once more after D1 lands on main. | D1 commits after the last check |

## Coverage rows

| U | R | Seam | Observation | Target and journey | Passes when | Fails for | Reuses | Unverified |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| U1 | R1 | GitHub run and job records | O1 | the PR run (base: D1's branch, so it runs D1's code) and the first main run after D1 and this change have both landed, on `ubuntu-latest`; `gh api …/runs/<id>/jobs`, lifetime `temporary` | max(required job `completed_at`) − run `created_at` ≤ 30 min on both runs; each required job's duration is recorded so a miss names the job that caused it | a run whose test job still has the late heavy tail (≈ 69 min on D1's green run 41994005347) | none | queue-heavy periods; the PR run's build reuses main's cache with a partial `Cargo.lock` match until D1 lands, so its build time overstates the post-merge cost |
| U2 | R2 | run job list | O1 | the same PR run | the PR run has ≤ 4 jobs (`static`, `test`, `e2e`, `wasm`) | a change that adds a partition job | none | — |
| U3 | R3 | same-revision list pair; CI Summary | O2 | at the PR head on the build machine: `cargo nextest list --profile ci`, with each invocation's exact `-p`/`--workspace`, `--exclude`, `--features` and `-E` selection, once with the PR's `.config/nextest.toml` and once with the pre-change file; plus the PR run's per-invocation `Summary`; lifetime `temporary` | for each invocation the run-identity sets are equal and the skipped-identity sets are equal, and, on Linux CI, the PR run's per-invocation run identity set and skip count equal those of main run 42023755753 at 614c03b75 (pre-change config; the changed tests keep their names); the build machine's list counts are never equated with Linux counts | a config that drops or skips any test, including one added after the merge base, because both lists come from the same revision | none | — |
| U3 | R4 | changed-test diff | O3 | the PR diff | each changed test keeps its fixture sizes, assertion set and deadline values; any changed wait names its causal signal | a deadline raised or a fixture shrunk | none | — |
| U4 | R5 | pre-fix and fixed under a controlled delayed start | O5, O2 | on the build machine, for each changed test: its pre-fix version and its fixed version, each with the fixture launch delayed past the test's deadline (for the known case, the Node fixture starting after more than 2 s); plus the PR run with threads following the CPU count; lifetime `temporary` | pre-fix fails the same way #213 did (`InitializeTimeout` for the known case); fixed passes with `Initialize(_)` and the process reaped; the CI run has 0 failures | a cosmetic "signal" that keeps the race (it still fails under the delayed start) | #213's saved red, as corroborating evidence for the known case | load cases not yet exposed |
| U5 | R6 | `static` job steps | O1 | the PR run | the step "Ratchet workspace module cycles" is present and succeeds | the step removed or skipped | none | — |
| U6 | R7 | merge-forward and per-selector resolution | O4 | D1's head each time it moves, and main after D1's squash lands; `git merge-tree` then a merge into the change's branch, and on the build machine each priority filter split into its member selectors and listed separately on the merged head; lifetime `temporary` | no conflict, and each member selector lists the identities it is meant to (exactly one for an exact name, the expected set for a prefix) on the merged head | a band that stays nonempty while one of its members was renamed by a later D1 commit | none | D1 commits after the last check |

## Shared observations

- **O1 · run timing and jobs:** read once per run from the GitHub API: creation time, each job's name and times, and its conclusion.
- **O2 · test identities and profile:** two inputs.
  - Same-revision `cargo nextest list` pairs, per invocation. These are the R3 oracle.
  - The CI log, parsed by `ci-test-profile` into one inventory per invocation (PASS and FAIL), grouped by nextest's own run total (`(<n>/<total>)`), with each inventory checked against the `Summary` reporting the same total. A run where the tool can't reproduce the Summary counts yields `inventoryMismatch`, and its profile is not used. The profile is evidence of where time goes, not the R1 verdict.
- **O3 · test diff:** the PR diff of every test and fixture file it touches, read against the pre-change file.
- **O4 · D1 merge-forward and selector check:** `git merge-tree` of `feat/central-storage-reset-d1-router` at its head (later, of main after D1's squash) against the change's branch, the merge itself (never a rebase), then the per-selector listings on the merged head.
- **O5 · delayed-start comparison:** each changed test is run twice under the same induced delay before its fixture starts, once in its pre-fix version and once in its fixed version, on the build machine.

## Limits

R1 may fail while R2–R5 all pass. That is the designed signal for owner question Q1, not a verification gap. The design does not claim R1 until O1 shows it.
