# CI to 30 minutes: Verification Design

Every requirement is proven on the real target: a GitHub-hosted CI run of the PR that carries the change, read through GitHub's own records and nextest's own output. Nothing here has been observed yet; this is the plan for what will be.

## Proven by (owner view)

| Need | Proven by | Stays unverified |
| --- | --- | --- |
| U1 · CI in 30 min | Two CI runs are timed: the PR's own run and the first main run after merge. Each is measured from GitHub's creation time to its last required check finishing, and compared with 30 minutes. The test step's profile shows where any time over 30 went. | Runs under heavy org-wide queueing; D1's own test mix until D1's first PR run |
| U2 · no extra machines | The PR run's job list is counted. It must not exceed 4. | — |
| U3 · same tests, same proof | At one commit, the tests run and skipped with the change are compared with those without it; nothing is missing. Each changed test's diff shows its fixtures, assertions and deadlines kept. | tests added by unrelated work in the meantime (they're named, not counted as loss) |
| U4 · causal waits | Each changed test names the signal it waits on and passes at 8 threads in CI. | load-sensitive tests no CI run has exposed yet |
| U5 · ratchet stays | The Static validation job still runs the ratchet step, and it passes. | — |
| U6 · works with D1 | The change merges into D1's current branch with no conflicts, and every priority filter matches tests on D1. | D1 commits after the check |

## Coverage rows

| U | R | Seam | Observation | Target and journey | Passes when | Fails for | Reuses | Unverified |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| U1 | R1 | GitHub run and job records | O1 | PR run and the next main run on `ubuntu-latest`; `gh api …/runs/<id>/jobs`, lifetime `temporary` | max(required job `completed_at`) − run `created_at` ≤ 30 min on both runs | a run whose test job still has the late heavy tail (≈ 54 min) | none | queue-heavy periods; D1's mix |
| U2 | R2 | run job list | O1 | the same PR run | the number of jobs in a PR run ≤ 4 (`static`, `test`, `e2e`, `wasm`) | a change that adds a partition job | none | — |
| U3 | R3 | nextest start line and PASS/SKIP identities | O2 | the PR run, and the same commit run without the change (main's run at the merge base, or #213's base for the 8-thread part); lifetime `temporary` | `Starting N tests … (M skipped)` gives equal N and M, and the set difference of identities is empty | a filter or config that drops or skips a test | none | tests added upstream between commits, listed separately |
| U3 | R4 | the diff of each changed test | O3 | the PR diff | each changed test keeps its fixture sizes, assertion set and deadlines, or replaces a wait with a named causal signal | a deadline raised, or a fixture shrunk | none | — |
| U4 | R5 | changed test + 8-thread run | O3, O2 | the PR run at `test-threads = 8`, lifetime `temporary` | each changed test names its awaited signal, and the run has 0 failures | a fix that only raises the 2 s initialize budget (the test passes, but O3 rejects the diff) | the existing test itself | load cases not yet exposed |
| U5 | R6 | `static` job steps | O1 | the PR run | the job has the step "Ratchet workspace module cycles", and its conclusion is success | the step removed or skipped | none | — |
| U6 | R7 | merge and filter resolution | O4 | D1's head when checked; `git merge-tree`, and `cargo nextest list --profile ci` on Sunclaw for each priority filter; lifetime `temporary` | no conflict, and every filter lists at least 1 test on main and on D1 | a filter naming a test D1 renamed | none | D1 commits after the check |

## Shared observations

- **O1 · run timing and jobs:** read once per run from the GitHub API, giving the run's creation time, each job's name and times, and its conclusion.
- **O2 · test identities and profile:** the test job's log, parsed by `ci-test-profile` into PASS identities, the start line and a `StepProfile`. The profile is evidence of where time goes, not the R1 verdict; O1 is the verdict. A version of the tool that miscounts would show up as a mismatch against nextest's own `Summary` line (6,518 run), which the tool must reproduce.
- **O3 · test diff:** the PR diff of every test file it touches, read against the pre-change file.
- **O4 · D1 merge check:** `git merge-tree` of the change against `feat/central-storage-reset-d1-router`, plus the filter listing.

## Limits

R1 may fail while R2–R5 all pass. That is the designed signal for owner question Q1, not a verification gap. The design does not claim R1 until O1 shows it.
