//! CS-1/CS-3 through owned Host processes, authenticated peers and the real Tokio central.

#![deny(clippy::too_many_lines)]
use crate::central_pair;
#[path = "../mail_reconcile/connected_status.rs"]
mod connected_status;

use crate::fixture;

use codex_router_test_support::FixtureResult;

#[tokio::test]
async fn connected_timer_repairs_lost_live_status_without_reconnect_or_another_send()
-> FixtureResult {
    central_pair::scenario(connected_status::lost_live_callback).await
}
