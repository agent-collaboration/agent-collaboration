# Central link client proof

Colocated `src/link_state_test.rs` cases cover lifecycle policy, one injected
clock, refusal/re-enrollment, capped reconnects, heartbeat misses, in-flight send
progress and obsolete connection events. `tests/integration/main.rs` exercises
the public client against a protocol stub through actual text encode/decode,
including multiple responses out of order and write identity across retry.

Run with `cargo nextest run -p central-link-client`. The stub stands in for an
authenticated central socket and proves no real socket, store, hibernation or
provider delivery behavior. Those gates remain with C2 and D.
