pub use codex_router_core::ids::AccountId;
use codex_router_selection::burn_down::RoutingReason;

use super::*;

#[path = "quota_status_formatting_test.rs"]
mod quota_status_formatting_test;

const NOW: u64 = 1_700_000_000;

#[path = "quota_refresh_provider_test.rs"]
mod quota_refresh_provider_test;
#[path = "quota_reset_pace_projection_test.rs"]
mod quota_reset_pace_projection_test;
#[path = "quota_route_selection_projection_test.rs"]
mod quota_route_selection_projection_test;
#[path = "quota_status_metrics_test.rs"]
mod quota_status_metrics_test;
use crate::test_support::*;
