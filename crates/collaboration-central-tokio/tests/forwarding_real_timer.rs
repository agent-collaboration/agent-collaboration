//! The production30s timer is a separate slow target; no pinned clock or short policy.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
#[allow(
    dead_code,
    reason = "Shared normal Fleet/Register fixture also covers unrelated creation refusals."
)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
#[allow(
    dead_code,
    reason = "The exact clock boundary runs separately from this actual timer target."
)]
#[path = "support/forwarded_post_deadline_boundaries.rs"]
mod forwarded_post_deadline_boundaries;
#[allow(
    dead_code,
    reason = "Original lifecycle oracles are retained and reused by several entrypoints."
)]
#[path = "support/forwarded_post_lifecycle_oracles.rs"]
mod forwarded_post_lifecycle_oracles;
#[path = "support/forwarded_post_native_cases.rs"]
mod forwarded_post_native_cases;
#[allow(
    dead_code,
    reason = "Shared forwarding scenario module includes fast origin and routing cases."
)]
#[path = "support/forwarded_post_oracles.rs"]
mod forwarded_post_oracles;
#[path = "support/forwarded_post_route_overlap.rs"]
mod forwarded_post_route_overlap;
#[allow(
    dead_code,
    reason = "Shared scenario enum serves native and Worker entrypoints."
)]
#[path = "support/forwarded_post_scenarios.rs"]
mod forwarded_post_scenarios;
#[path = "support/forwarded_post_state_cases.rs"]
mod forwarded_post_state_cases;
#[path = "support/forwarded_post_state_oracles.rs"]
mod forwarded_post_state_oracles;
#[path = "support/forwarded_post_terminal_slots.rs"]
mod forwarded_post_terminal_slots;
#[path = "support/forwarded_post_wire.rs"]
mod forwarded_post_wire;
#[allow(
    dead_code,
    reason = "Shared owner HTTP helper also contains independent real writer scenarios."
)]
#[path = "support/holder_writer_retry.rs"]
mod holder_writer_retry;

#[tokio::test]
async fn production_alarm_returns_unanswered_and_releases_all_capacity_for_fresh_correlations()
-> collaboration_central_conformance::ConformanceResult<()> {
    forwarded_post_native_cases::run(forwarded_post_scenarios::ForwardCase::RealTimer).await
}
