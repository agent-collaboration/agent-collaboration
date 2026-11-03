//! The production30s timer is a separate slow target; no pinned clock or short policy.
#![cfg(feature = "conformance")]

use crate::forwarded_post_native_cases;

use crate::forwarded_post_scenarios;

#[tokio::test]
async fn production_alarm_returns_unanswered_and_releases_all_capacity_for_fresh_correlations()
-> collaboration_central_conformance::ConformanceResult<()> {
    forwarded_post_native_cases::run(forwarded_post_scenarios::ForwardCase::RealTimer).await
}
