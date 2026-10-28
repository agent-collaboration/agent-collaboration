//! VD §0/OB1 and §5a board contracts through real Host/CLI/MCP listeners.

#![deny(clippy::too_many_lines)]
#[path = "board_e2e/complete_pages.rs"]
mod complete_pages;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/fixture.rs"]
mod fixture;
#[path = "board_e2e/fixture_inputs.rs"]
mod fixture_inputs;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/follow_fixture.rs"]
mod follow_fixture;
#[path = "board_e2e/held_retention.rs"]
mod held_retention;
#[path = "board_e2e/list_subjects.rs"]
mod list_subjects;
#[path = "board_e2e/membership_restart.rs"]
mod membership_restart;
#[path = "board_e2e/notification_peer.rs"]
mod notification_peer;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/owned_host.rs"]
mod owned_host;
#[path = "board_e2e/page_recovery.rs"]
mod page_recovery;
#[path = "board_e2e/poll_delivery.rs"]
mod poll_delivery;
#[path = "board_e2e/record_inputs.rs"]
mod record_inputs;
#[path = "board_e2e/records_reopen.rs"]
mod records_reopen;
#[path = "board_e2e/records_roles.rs"]
mod records_roles;
#[path = "board_e2e/records_subjects.rs"]
mod records_subjects;
#[path = "board_e2e/records_views.rs"]
mod records_views;
#[path = "board_e2e/records_work.rs"]
mod records_work;
#[path = "board_e2e/registration_orphan.rs"]
mod registration_orphan;
#[path = "board_e2e/rejected_scopes.rs"]
mod rejected_scopes;
#[path = "board_e2e/repository_fixture.rs"]
mod repository_fixture;
#[path = "board_e2e/role_checks.rs"]
mod role_checks;
#[path = "board_e2e/role_notifications.rs"]
mod role_notifications;
#[path = "board_e2e/scope_inbox.rs"]
mod scope_inbox;
#[path = "board_e2e/stopped_faults.rs"]
mod stopped_faults;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/takeover_readbacks.rs"]
mod takeover_readbacks;
use codex_router_test_support::FixtureResult;
use fixture::scenario;
use serde_json::json;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn real_host_central_and_cli_mcp_share_the_persisted_installation() -> FixtureResult {
    // Wrong builds: either listener returns an empty list, or owning restart loses records/identity.
    scenario(async |suite| {
        const WORKSPACE: &str = "019f0000-0000-7000-8000-000000001205";
        const TITLE: &str = "persisted installation workspace";
        suite.accept("workspace_create", fixture::fields(fixture::header(&suite.owner),
            json!({"workspaceId":WORKSPACE,"name":TITLE,"description":"literal persisted smoke"}))?).await?;
        for reopening in [false, true] {
            if reopening {
                suite.stop_host().await?;
                suite.reopen_host().await?;
            }
            assert_eq!(suite.host()?.mcp("installation_show", json!({})).await?,
                serde_json::to_value(suite.installation.as_ref().ok_or("installation identity absent")?)?);
            let cli = suite.cli(&["workspace", "list"]).await?;
            let mcp = suite.host()?.mcp("workspace_list",
                json!({"repository":null,"page":{"limit":50,"cursor":null}})).await?;
            assert_eq!(cli, mcp);
            let records = fixture::at(&cli, "/page/records")?.as_array().ok_or("workspace records absent")?;
            assert_eq!(records.len(), 1, "the literal workspace must remain visible");
            let workspace = records.first().ok_or("workspace absent")?;
            assert_eq!(workspace.get("workspaceId"), Some(&json!(WORKSPACE)));
            assert_eq!(workspace.get("name"), Some(&json!(TITLE)));
            assert_eq!(cli.pointer("/page/nextCursor"), Some(&serde_json::Value::Null));
        }
        Ok(())
    }).await
}

#[tokio::test]
async fn role_authority_cardinality_and_history_survive_owning_reopen() -> FixtureResult {
    scenario(records_roles::authority_and_cardinality).await
}

#[tokio::test]
async fn all_thread_subjects_refile_replay_and_current_titles_survive_reopen() -> FixtureResult {
    scenario(records_subjects::subjects_and_refiling).await
}

#[tokio::test]
async fn keyed_work_and_causal_late_facts_survive_reopen() -> FixtureResult {
    scenario(records_work::replay_keyed_work_and_late_facts).await
}

#[tokio::test]
async fn pending_views_settle_revive_and_isolate_foreign_metadata_without_marking_reads()
-> FixtureResult {
    scenario(records_views::pending_views).await
}

#[tokio::test]
async fn foreign_thread_watch_keeps_context_while_plain_project_watch_adds_no_coverage()
-> FixtureResult {
    scenario(scope_inbox::foreign_watch).await
}

#[tokio::test]
async fn duplicate_semantic_ack_refuses_before_either_request_key_or_unread_coverage()
-> FixtureResult {
    scenario(scope_inbox::semantic_ack).await
}

#[tokio::test]
async fn complete_posts_participants_and_threads_cross_both_listeners_and_survive_corrupt_sibling()
-> FixtureResult {
    scenario(complete_pages::complete_pages).await
}

#[tokio::test]
async fn corrupt_membership_keeps_the_same_readers_healthy_project_and_owed_poll() -> FixtureResult
{
    scenario(membership_restart::corrupt_sibling).await
}

#[tokio::test]
async fn unavailable_workspace_sibling_preserves_healthy_rows_and_explicit_refusal() -> FixtureResult
{
    scenario(page_recovery::unavailable_sibling).await
}

#[tokio::test]
async fn aggregate_cursor_keeps_unavailable_member_positions_until_recovery() -> FixtureResult {
    scenario(page_recovery::aggregate_cursor_recovery).await
}

#[tokio::test]
async fn poll_ranges_settle_once_and_keep_policy_identity_and_owed_work_on_reopen() -> FixtureResult
{
    scenario(poll_delivery::poll_restart).await
}

#[tokio::test]
async fn each_live_reviewer_receives_its_own_frozen_notification_before_and_after_replacement()
-> FixtureResult {
    scenario(role_notifications::two_reviewers).await
}

#[tokio::test]
#[ignore = "Owned subprocess entrypoint; invoked by the non-ignored role notification scenario."]
async fn board_notification_peer_process() -> FixtureResult {
    notification_peer::run_child().await
}

#[tokio::test]
async fn repository_default_lists_preserve_current_subjects_and_hide_redacted_or_missing_sources()
-> FixtureResult {
    scenario(list_subjects::repository_defaults_and_privacy).await
}

#[tokio::test]
async fn rejected_native_scopes_never_enter_healthy_aggregate_reads_or_session_context()
-> FixtureResult {
    scenario(rejected_scopes::rejected_scope_and_context).await
}

#[tokio::test]
async fn held_follow_payload_outlives_real_terminal_history_pruning_and_delivers_original_range_once()
-> FixtureResult {
    scenario(held_retention::held_follow_survives_history_pruning).await
}
