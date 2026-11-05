# Plan: CI to 30 minutes (scheduling slice), threads follow CPUs

```text
plan path: docs/specs/2026-11-04-ci-30-minutes/plans/2026-11-05-ci-30-minutes-threads-follow-cpus.md
originating planner: plan-update
supersedes: docs/specs/2026-11-04-ci-30-minutes/plans/2026-11-05-ci-30-minutes-on-d1.md
planning result: ready
governing planning basis:
  kind: reviewed-four-document-design
  Requirements:        docs/specs/2026-11-04-ci-30-minutes/requirements.md
  Specification:       docs/specs/2026-11-04-ci-30-minutes/specification.md
  Program Design:      docs/specs/2026-11-04-ci-30-minutes/program-design.md (thread rationale and the oversized-frame seam corrected 2026-11-05)
  Verification Design: docs/specs/2026-11-04-ci-30-minutes/verification-design.md (U3/R3 Linux comparison corrected 2026-11-05)
  review: four-document-design review, ready; corrected anchors verified by the same Reviewer; implementation review of PR #217 at a7adfbea9 returned two findings, corrected in this pass
  current applicability anchors: main e34ebef87
delivery context:
  requested terminal: pr-ready-unmerged
  delivery grouping: single:scheduling
  PR topology: one-pr (PR #217, base main)
  repositories: agent-collaboration/agent-collaboration
```

## Changed from the 2026-11-05 on-D1 plan

- **Threads.** `[profile.ci] test-threads` follows the CPU count (`"num-cpus"`), not 8. On the same post-D1 code, 8 threads on the 4-vCPU runner saved about 1.5 minutes: the workspace test step went from 28.7 to 27.2 minutes. It also starved capped causal waits: four runs gave four different load failures, and run 42062794131's workspace step took 51.5 minutes. The 8-thread choice was the Lead's assumption, not an owner decision. It invalidates S3's thread line and the goal's "run 8 threads"; nothing else changes.
- **Oversized-frame seam (S2).**
  - **Clock release:** the watcher releases the paused clock when the frame-written marker exists, or when the child whose process ID was recorded at startup has exited.
  - **Proof of cause:** the consumer-side proof that the oversized frame caused the failure is the frame-limit counter, which acp-client-runtime exposes under its existing `test-observation` feature.
  - **Reason:** this answers implementation-review finding F1.
- **Profiler (S1).** A started invocation with no Summary returns `noTimings` / `summaryNotFound`, as review finding F2 asked.
- **S4.** D1 has landed, so the branch merges main forward. No rebase.
- **D1 files.** The four causal fixes in D1 code were reviewed by the D1 Lead.

Everything else carries forward from the on-D1 plan unchanged: slices S1–S5, edges, obligation map, gates, risks and stop conditions. R1 is still expected to miss on 4 vCPU. That is the designed Q1 signal, and the runner choice sits with the owner.
