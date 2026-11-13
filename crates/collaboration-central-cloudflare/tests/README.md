# collaboration-central-cloudflare test contracts

This package declares no Rust integration or e2e test target. Its unit tests stay
colocated with their implementation (`src/*_test.rs`).

- `public-template.test.mjs`: the public deployment template keeps its placeholders
  and binds `FLEET` and `WORKSPACE_STORE` to SQLite Durable Object classes. Run from
  the root with `node --test crates/collaboration-central-cloudflare/tests/public-template.test.mjs`.
- The non-default `test-support` feature compiles the storage boundary's workerd
  case routes (`/test-support/...`), the moved SQLx driver's scenarios
  (`/test-support/driver/`) and their `StorageCaseObject` and `DriverScenarioObject`
  classes into a
  separate test bundle (`build-test-support/`). The cases themselves live in
  `verification/targets/workerd-central-storage.test.ts`; the production bundle check
  refuses a bundle that carries any of these routes.

The O1 bundle is built with `--features test-support,conformance`. Its actual-object
clock case calls the real `FleetObject` clock control and `IdSource`, checks held
and advanced UUIDv7 milliseconds, and fails if the route is absent. The route is
compiled only when both test features are enabled and is rejected by the production
bundle marker check.
