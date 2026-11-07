# Test organization phase 1: Lead disposition for the S7 baseline

originating planner: plan-update
supersedes: plans/2026-11-06-test-organization-phase-1-dispositions.md (adds D4 only; D1–D3 and every other slice, edge, gate and proof carry forward unchanged)
governing basis: the reviewed four-document design (ready at e4825ae32, O5 clarified at ad7c4b38c)
requested terminal: pr-ready-unmerged, one PR (unchanged)

## Changed from

### D4 · S7 measures the extraction on its own base, then proves integration against current main

- **Found:** main moved after P and the two before samples were taken. Two merged PRs add Host and central-core test cases, bump the workspace version and change the lockfile. Merging main before measuring would mix those upstream changes into both O4 and O5.
- **What the design asks:** O4 compares "the pinned pre-extraction tree P and the post-extraction tree Q". R5 judges "the change being measured", which is the extraction. Both are about the move, not about whatever main holds when the PR opens.
- **Disposition:** keep the original experiment; don't refresh the baseline.
  1. **O4 on the original base.** Collect Q on the branch head before merging main, from the same base P was taken on, with the same toolchain and the six selections. For each selection, Q's run and skipped identity sets equal P's. This is the R4 proof.
  2. **O5 on the original base.** Take the two after samples a1 and a2 from that same pre-merge head on `ci/test-org-measure`, with the same machine type, scenarios, cache rule and selection as b1 and b2. Apply the unchanged R5 rule against the original b1 and b2.
  3. **Then merge main forward.** Consume the upstream changes as they are: no repair, revert or B2 edit.
  4. **Integration proof on the merged head.** For each of the six selections, the merged head's run and skipped identity sets equal current main's (the main commit merged in), listed with the same toolchain. That proves the extraction drops no case that main added, and adds none. O1, O2 (local native view) and O3 rerun on the merged head. The PR's own CI run is the final green.
- **Why not refresh:** it costs two more four-vCPU jobs (about 43 minutes each). It also restarts the race, because main can move again before the after samples finish. A fixed base is what makes the attribution exact.
- **Not allowed:** an added-case allowance in O4, comparing merged-head timings with the original before samples, or rewriting P.
- **Stop if:** the original-base Q differs from P in any selection, R5 fails, or the merged head's inventory differs from current main's in a way that isn't a merge conflict you resolved by keeping both sides.

## Unchanged

S7's remaining order is: O4 and O5 on the original base, merge main, integration proof, push, then a draft PR. After that comes the independent Sol xhigh implementation review before merge. `ci/test-org-measure` is never merged.
