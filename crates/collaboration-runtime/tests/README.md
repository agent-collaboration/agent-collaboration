# collaboration-runtime test contracts

This package declares one integration target, `tests/integration` (below), and
no e2e target. Unit tests remain colocated with their implementation; scheduler failure/deadline cases are grouped
under `schedule_timing_worker::tests::retry_tests`.
`summary_native_worker::tests` holds a real native socket response and proves
the separate thirty-second summary request bound using that same ManualClock.
Shared fixtures belong to their named test-support owners. Package and platform-specific checks remain
in the workspace CI selection and the owning source modules.

The converted worker and retention tests advance one shared `ManualClock` and wait
on causal observations. The CI profile supplies their outer hang bound:

```sh
cargo nextest run --profile ci -p collaboration-runtime
```

## tests/integration

- `turmoil_link`: the production central link driver (RouterLinkCalls, the
  Mac outbox store and the minute flush) over a Turmoil-simulated network and
  clock from `collaboration-link-test-support`, against a central stub that
  keeps a request log. Proves every write
  the Router accepted reaches central exactly once through partitions, lost
  answers and a Router restart.
