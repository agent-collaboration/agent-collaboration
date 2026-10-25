//! CS-1/CS-3 through owned Host processes, authenticated peers and the real Tokio central.
#![allow(clippy::panic_in_result_fn)]
#![deny(clippy::too_many_lines)]
#[allow(dead_code)]
#[path = "board_e2e/central_pair.rs"]
mod central_pair;
#[path = "mail_reconcile/connected_status.rs"]
mod connected_status;
#[allow(dead_code)]
#[path = "board_e2e/fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "board_e2e/fixture_inputs.rs"]
mod fixture_inputs;
#[allow(dead_code)]
#[path = "board_e2e/owned_host.rs"]
mod owned_host;
use codex_router_test_support::FixtureResult;

#[tokio::test]
async fn connected_timer_repairs_lost_live_status_without_reconnect_or_another_send()
-> FixtureResult {
    central_pair::scenario(connected_status::lost_live_callback).await
}
