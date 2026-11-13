# sqlx-durable-object tests

- `integration/` (target `integration`): host-side compile-time proof that a consumer
  of the `sqlx` facade sees this crate's `Do` as its `sqlx::Database`, with `Send`
  futures. There is no Durable Object on the host, so nothing here runs SQL.
- Unit tests stay colocated in `src/*_test.rs`.

The driver's scenarios that need a real SQLite-backed Durable Object moved with the
driver and now run inside the central Worker's `test-support` bundle
(`crates/collaboration-central-cloudflare/src/driver_scenarios.rs`, route
`/test-support/driver/`). `verification/targets/workerd-central-storage.test.ts`
walks every scenario under the pinned workerd:

```sh
worker-build crates/collaboration-central-cloudflare --dev --no-opt --out-dir build-test-support -- --features test-support,conformance
pnpm --dir verification run test:workerd
```

One upstream scenario was dropped in the move: `transaction_holds_off_other_requests`.
It held a transaction open across a second request by awaiting 50 ms and 300 ms
timers, which is a wall-clock test (AGENTS.md "Time"); it passed only at the
upstream Worker's 2026-01-01 compatibility date, and the central Worker ships at a
later one where workerd cancels a timer awaited inside `storage.transaction`; and
central never relies on that serialization: its per-workspace write turn
(`collaboration-central-core` `WorkspaceTurns`) orders requests, and no storage
path awaits a timer inside a transaction.
