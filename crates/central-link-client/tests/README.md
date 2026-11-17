# Central link client proof

Colocated `src/link_state_test.rs` cases cover lifecycle policy, one injected
clock, refusal/re-enrollment, capped reconnects, heartbeat misses, in-flight send
progress and obsolete connection events. `tests/integration/main.rs` exercises
the public client against a protocol stub through actual text encode/decode,
including multiple responses out of order and write identity across retry.

Run with `cargo nextest run -p central-link-client`. The stub stands in for an
authenticated central socket and proves no real socket, store, hibernation or
provider delivery behavior. Those gates remain with C2 and D.

`src/link_liveness_test.rs` is a child of the existing unit aggregate, sharing
its fixture helpers. It covers individual 30-second call deadlines, a ten-second
Welcome deadline, safe default send classification, typed late replies,
actual-probe heartbeat counting and close/refuse ordering. Eight cases were
first run against the old implementation and failed at their intended assertions.

Below the seam, colocated Hegel tests cover: `submission_path_test.rs` (every
operation sample against the owner's outbox kinds, with serde's variant list
guarding that no operation is missing); `flush_marks_test.rs` (the ±3 s window
and the open mark); `outbox_flush_test.rs` (a stateful machine: nothing lost,
nothing sent twice while awaiting its answer, nothing before its mark, at most
64 per batch; and a drain property that a connected link empties the outbox at
64 per mark). Seeded wrong builds (send before the mark; drop in-flight members
on link loss; classify SetWake as outbox) each fail these tests.
`fake_link_calls_test.rs` covers the stand-in's contract.
