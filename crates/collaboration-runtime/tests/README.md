# collaboration-runtime test contracts

This package declares no integration or e2e test target. Unit tests remain
colocated with their implementation. `interaction_retention_worker` groups its
schedule, fault and history cases under its own folder; each case runs the
worker on real interaction stores.
Shared fixtures belong to their named test-support owners; this folder adds no
empty aggregate or executable journey. Package and platform-specific checks remain
in the workspace CI selection and the owning source modules.

The worker and retention tests advance one shared `ManualClock` and wait on
causal observations. The CI profile supplies their outer hang bound:

```sh
cargo nextest run --profile ci -p collaboration-runtime
```
