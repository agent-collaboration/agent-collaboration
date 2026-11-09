# collaboration-runtime test contracts

This package declares no integration or e2e test target. Unit tests remain
colocated with their implementation; scheduler failure/deadline cases are grouped
under `schedule_timing_worker::tests::retry_tests`.
Shared fixtures belong to their named test-support owners; this folder adds no
empty aggregate or executable journey. Package and platform-specific checks remain
in the workspace CI selection and the owning source modules.

The converted worker and retention tests advance one shared `ManualClock` and wait
on causal observations. The CI profile supplies their outer hang bound:

```sh
cargo nextest run --profile ci -p collaboration-runtime
```
