//! VD §0/OB1 and §5a board contracts through real Host/CLI/MCP listeners.

#![deny(clippy::too_many_lines)]
#[path = "../board_e2e/complete_pages.rs"]
mod complete_pages;

#[path = "../board_e2e/closed_peer_refusal.rs"]
mod closed_peer_refusal;
#[path = "../board_e2e/held_retention.rs"]
mod held_retention;
#[path = "../board_e2e/list_subjects.rs"]
mod list_subjects;
#[path = "../board_e2e/membership_restart.rs"]
mod membership_restart;
#[path = "../board_e2e/notification_peer.rs"]
mod notification_peer;
#[path = "../board_e2e/page_recovery.rs"]
mod page_recovery;
#[path = "../board_e2e/poll_delivery.rs"]
mod poll_delivery;
#[path = "../board_e2e/record_inputs.rs"]
mod record_inputs;
#[path = "../board_e2e/records_roles.rs"]
mod records_roles;
#[path = "../board_e2e/records_subjects.rs"]
mod records_subjects;
#[path = "../board_e2e/records_views.rs"]
mod records_views;
#[path = "../board_e2e/records_work.rs"]
mod records_work;
#[path = "../board_e2e/registration_orphan.rs"]
mod registration_orphan;
#[path = "../board_e2e/rejected_scopes.rs"]
mod rejected_scopes;
#[path = "../board_e2e/repository_fixture.rs"]
mod repository_fixture;
#[path = "../board_e2e/role_checks.rs"]
mod role_checks;
#[path = "../board_e2e/role_notifications.rs"]
mod role_notifications;
#[path = "../board_e2e/scope_inbox.rs"]
mod scope_inbox;
#[path = "../board_e2e/stopped_faults.rs"]
mod stopped_faults;

use codex_router_test_support::FixtureResult;

#[tokio::test]
#[ignore = "Owned subprocess entrypoint; invoked by the non-ignored role notification scenario."]
async fn board_notification_peer_process() -> FixtureResult {
    notification_peer::run_child().await
}
