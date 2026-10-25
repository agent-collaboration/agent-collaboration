//! Retained native behavioral claims use the existing crate-local deterministic admission seam.
#[path = "behavior_tests/subscription_fixture.rs"]
mod subscription_fixture;
use chrono::Duration;
use message_board::*;
use subscription_fixture::*;
#[path = "behavior_tests/activity_boundaries.rs"]
mod activity_boundaries;
#[path = "behavior_tests/covered_watches.rs"]
mod covered_watches;
#[path = "behavior_tests/intent_handoff.rs"]
mod intent_handoff;
#[path = "behavior_tests/membership_completion.rs"]
mod membership_completion;
#[path = "behavior_tests/membership_lifecycle.rs"]
mod membership_lifecycle;
#[path = "behavior_tests/membership_renewal.rs"]
mod membership_renewal;
use crate::native_worker_faults;
#[path = "behavior_tests/project_watch_fidelity.rs"]
mod project_watch_fidelity;

#[path = "behavior_tests/activation_boundaries.rs"]
mod activation_boundaries;
#[path = "behavior_tests/archived_membership.rs"]
mod archived_membership;
#[path = "behavior_tests/flight_regressions.rs"]
mod flight_regressions;
#[path = "behavior_tests/membership_corruption.rs"]
mod membership_corruption;
#[path = "behavior_tests/membership_drain.rs"]
mod membership_drain;
#[path = "behavior_tests/participant_corruption.rs"]
mod participant_corruption;
#[path = "behavior_tests/participant_delivery.rs"]
mod participant_delivery;
#[path = "behavior_tests/participant_gates.rs"]
mod participant_gates;
#[path = "behavior_tests/participant_intervals.rs"]
mod participant_intervals;
#[path = "behavior_tests/participant_lifecycle.rs"]
mod participant_lifecycle;
#[path = "behavior_tests/post_cooldown.rs"]
mod post_cooldown;
#[path = "behavior_tests/project_coverage.rs"]
mod project_coverage;
#[path = "behavior_tests/project_notice_boundaries.rs"]
mod project_notice_boundaries;
#[path = "behavior_tests/reactivation_regressions.rs"]
mod reactivation_regressions;
#[path = "behavior_tests/read_agreement.rs"]
mod read_agreement;
#[path = "behavior_tests/window_notices.rs"]
mod window_notices;
#[path = "behavior_tests/window_recovery.rs"]
mod window_recovery;
#[path = "behavior_tests/window_release.rs"]
mod window_release;
#[path = "behavior_tests/window_selection_fences.rs"]
mod window_selection_fences;
#[path = "behavior_tests/window_settlement.rs"]
mod window_settlement;
#[path = "behavior_tests/window_settlement_storage.rs"]
mod window_settlement_storage;
#[path = "behavior_tests/window_timing.rs"]
mod window_timing;
