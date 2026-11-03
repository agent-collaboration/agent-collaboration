//! The production30s alarm runs from controlled time through the normal socket path.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;

#[path = "support/forwarded_post_deadline_boundaries.rs"]
mod forwarded_post_deadline_boundaries;

#[path = "support/forwarded_post_lifecycle_oracles.rs"]
mod forwarded_post_lifecycle_oracles;
#[path = "support/forwarded_post_native_cases.rs"]
mod forwarded_post_native_cases;

#[path = "support/forwarded_post_oracles.rs"]
mod forwarded_post_oracles;
#[path = "support/forwarded_post_route_overlap.rs"]
mod forwarded_post_route_overlap;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
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

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/holder_writer_retry.rs"]
mod holder_writer_retry;

#[tokio::test]
async fn production_alarm_returns_unanswered_and_releases_all_capacity_for_fresh_correlations()
-> collaboration_central_conformance::ConformanceResult<()> {
    forwarded_post_native_cases::run(forwarded_post_scenarios::ForwardCase::AlarmEvent).await
}
