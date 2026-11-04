# CI to 30 minutes: Verification Design

Every requirement is proven on the real target: a GitHub-hosted CI run of the PR that carries the change, read through GitHub's own records and nextest's own output, plus same-revision listings and controlled test runs on Sunclaw. Nothing here has been observed yet; this is the plan for what will be.

## Proven by (owner view)

| Need | Proven by | Stays unverified |
| --- | --- | --- |
| U1 · CI in 30 min | Two CI runs are timed: the PR's own run and the first main run after merge. Each is measured from GitHub's creation time to its last required check finishing, and compared with 30 minutes. Each invocation's profile shows where any time over 30 went. | runs under heavy org-wide queueing; D1's own test mix until D1's first PR run |
| U2 · no extra machines | The PR run's job list is counted. It must not exceed 4. | — |
| U3 · same tests, same proof | At one source revision, nextest lists the tests it would run and skip, with the new config and with the old one, for each invocation. The two lists must be identical, and the CI run's own counts must match its list. Each changed test's diff shows its fixtures, assertions and deadlines kept. | — |
| U4 · causal waits | Each changed test is run with its fixture's start deliberately delayed, both before the fix (it fails) and after (it passes). The CI run at 8 threads also has no failures. | load-sensitive tests no CI run has exposed yet |
| U5 · ratchet stays | The Static validation job still runs the ratchet step, and it passes. | — |
| U6 · works with D1 | The change merges into D1's current branch with no conflicts. Each test a priority rule names is listed separately on main and on D1 and found. | D1 commits after the check |

## Coverage rows

| U | R | Seam | Observation | Target and journey | Passes when | Fails for | Reuses | Unverified |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| U1 | R1 | GitHub run and job records | O1 | the PR run and the next main run on `ubuntu-latest`; `gh api …/runs/<id>/jobs`, lifetime `temporary` | max(required job `completed_at`) − run `created_at` ≤ 30 min on both runs | a run whose test job still has the late heavy tail (≈ 54 min) | none | queue-heavy periods; D1's mix |
| U2 | R2 | run job list | O1 | the same PR run | the PR run has ≤ 4 jobs (`static`, `test`, `e2e`, `wasm`) | a change that adds a partition job | none | — |
| U3 | R3 | same-revision list pair; CI Summary | O2 | at the PR head on Sunclaw: `cargo nextest list --profile ci`, with each invocation's exact `-p`/`--workspace`, `--exclude`, `--features` and `-E` selection, once with the PR's `.config/nextest.toml` and once with the pre-change file; plus the PR run's per-invocation `Summary`; lifetime `temporary` | for each invocation the run-identity sets are equal and the skipped-identity sets are equal, and the CI `Summary` run/skip counts equal the new list's counts | a config that drops or skips any test, including one added after the merge base, because both lists come from the same revision | none | — |
| U3 | R4 | changed-test diff | O3 | the PR diff | each changed test keeps its fixture sizes, assertion set and deadline values; any changed wait names its causal signal | a deadline raised or a fixture shrunk | none | — |
| U4 | R5 | pre-fix and fixed under a controlled delayed start | O5, O2 | on Sunclaw, for each changed test: its pre-fix version and its fixed version, each with the fixture launch delayed past the test's deadline (for the known case, the Node fixture starting after more than 2 s); plus the PR run at 8 threads; lifetime `temporary` | pre-fix fails the same way #213 did (`InitializeTimeout` for the known case); fixed passes with `Initialize(_)` and the process reaped; the CI run has 0 failures | a cosmetic "signal" that keeps the race (it still fails under the delayed start) | #213's saved red, as corroborating evidence for the known case | load cases not yet exposed |
| U5 | R6 | `static` job steps | O1 | the PR run | the step "Ratchet workspace module cycles" is present and succeeds | the step removed or skipped | none | — |
| U6 | R7 | merge and per-selector resolution | O4 | D1's head when checked; `git merge-tree`, and on Sunclaw each priority filter split into its member selectors and listed separately on main and on D1; lifetime `temporary` | no conflict, and each member selector lists the identities it is meant to (exactly one for an exact name, the expected set for a prefix) on both branches | a band that stays nonempty while one of its members was renamed on D1 | none | D1 commits after the check |

## Shared observations

- **O1 · run timing and jobs:** read once per run from the GitHub API: creation time, each job's name and times, and its conclusion.
- **O2 · test identities and profile:** two inputs.
  - Same-revision `cargo nextest list` pairs, per invocation. These are the R3 oracle.
  - The CI log, parsed by `ci-test-profile` into one inventory per invocation (PASS and FAIL), with each inventory checked against that invocation's own `Summary`. A run where the tool can't reproduce the Summary counts yields `inventoryMismatch`, and its profile is not used. The profile is evidence of where time goes, not the R1 verdict.
- **O3 · test diff:** the PR diff of every test and fixture file it touches, read against the pre-change file.
- **O4 · D1 merge and selector check:** `git merge-tree` against `feat/central-storage-reset-d1-router` at its head, plus the per-selector listings.
- **O5 · delayed-start comparison:** each changed test is run twice under the same induced delay before its fixture starts, once in its pre-fix version and once in its fixed version, on Sunclaw.

## Limits

R1 may fail while R2–R5 all pass. That is the designed signal for owner question Q1, not a verification gap. The design does not claim R1 until O1 shows it.
