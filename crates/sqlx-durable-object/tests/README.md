# sqlx-durable-object tests

- `integration/` (target `integration`): host-side compile-time proof that a consumer
  of the `sqlx` facade sees this crate's `Do` as its `sqlx::Database`, with `Send`
  futures. There is no Durable Object on the host, so nothing here runs SQL.
- `worker/` (crate `sqlx-durable-object-test-worker`, `publish = false`): the driver's
  scenario Worker, moved with the driver. `GET /` lists the scenarios and
  `GET /<scenario>` runs one inside a real SQLite-backed Durable Object and answers
  `{"ok": true}` or `{"ok": false, "error": "..."}`. Reason on record for keeping a
  separate crate: it is a `cdylib` Worker with its own Durable Object class, which a
  host test target cannot be; it was a separate crate before the move and keeps every
  scenario unchanged.

The scenario Worker runs under the pinned workerd through the verification package
(it replaces the former `wrangler dev` script and `ureq` driver, neither of which this
repository pins):

```sh
worker-build crates/sqlx-durable-object/tests/worker --dev
pnpm --dir verification run test:workerd
```

Unit tests stay colocated in `src/*_test.rs`.
