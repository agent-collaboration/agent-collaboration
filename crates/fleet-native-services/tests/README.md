# Native fleet service test contracts

`integration` covers public APIs and storage contracts without launching the
consumer process. It retains `legacy_delivery` and `migration_stack`, adds 15
consumer cases moved from agent-fleet-sync b9470dc, and independently checks the
consumer's generated query corpus against the native schema.

`e2e` contains 82 moved consumer journeys. These use the real FleetService,
loopback transport, native Turso, writer and delivery-node processes, including
kill/restart recovery. The hard-link case is a supporting filesystem probe.
Each case runs in its own nextest process; no case is retried.

Both moved suites require `test-support,test-fault`. The two same-package bin
targets `fleet-sync-test-consumer` and `delivery_node` use the shared fixture
modules under `support/consumer`; process helpers live under `support/process`.
The Cargo signing runner signs those direct child artifacts before tests launch
on macOS. All state is fixture-owned; the production Router is not involved.

From the repository root:

```sh
cargo nextest run --locked -p fleet-native-services --features test-support,test-fault --test integration --test e2e
```

The consumer query manifest is `consumer-checked-queries.json`. The build script
checks its committed output and metadata; `consumer_checked_queries` also runs
the pinned generator's native describe/check without writing output. Fixture
SQL and migration bundles under `support/consumer` are not product schemas.

Workerd fixture exports retain the original environment variables:
`NORMAL_RUN_ROOT`, `NORMAL_FIXTURE_ROOT`, `COPY_CERTAINTY_FIXTURE` and
`FK_CAPTURE_FIXTURE`. The five rename cases export raw Piece JSON with exact
wide integers. Their separate workerd replay belongs to fleet-project-copy;
native passes do not substitute for workerd evidence.
