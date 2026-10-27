//! CS-1/CS-3 through owned Host processes, authenticated peers and the real Tokio central.

#![deny(clippy::too_many_lines)]

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/central_pair.rs"]
mod central_pair;
#[path = "mail_reconcile/connected_status.rs"]
mod connected_status;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/fixture.rs"]
mod fixture;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/fixture_inputs.rs"]
mod fixture_inputs;
#[path = "mail_reconcile/local_cancel.rs"]
mod local_cancel;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/owned_host.rs"]
mod owned_host;
use codex_router_test_support::FixtureResult;

#[tokio::test]
async fn local_cancellation_reports_remote_original_prevention_and_replay_never_injects()
-> FixtureResult {
    central_pair::scenario(local_cancel::remote_original).await
}

#[tokio::test]
async fn connected_timer_repairs_lost_live_status_without_reconnect_or_another_send()
-> FixtureResult {
    central_pair::scenario(connected_status::lost_live_callback).await
}
