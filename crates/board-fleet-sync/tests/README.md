# board-fleet-sync tests

Run the package with its workspace CI features:

```sh
cargo nextest run --locked --profile ci -p board-fleet-sync
```

`integration/main.rs` gathers 10 former external test roots.
Cases cover board project storage, cursor progress, and fleet session transport. Library unit tests remain colocated with their implementations.

`reqwest_session_transport_test.rs` uses paused Tokio time and a controlled
socket to cover idle closure, Ping cadence across receive cancellation, contact
from binary/Ping/Pong frames, and the real session pump's reconnect path.

Each aggregate declares shared helper implementations once. Existing fixture
directories remain in place. Only nonempty test categories are declared in
Cargo.toml; CI selects no separate e2e category for this package.

A former binary `<root>` and test `<case>` becomes its category binary and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
