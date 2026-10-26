//! VD §0/OB1 and §5a board contracts through real Host/CLI/MCP listeners.
#![allow(clippy::panic_in_result_fn)]
#![deny(clippy::too_many_lines)]
#[path = "board_e2e/complete_pages.rs"]
mod complete_pages;
#[path = "board_e2e/fixture.rs"]
mod fixture;
#[path = "board_e2e/fixture_inputs.rs"]
mod fixture_inputs;
#[allow(
    dead_code,
    reason = "Shared follow fixture also serves takeover cancellation and sender-status cases."
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
#[allow(
    dead_code,
    reason = "Shared stopped-owner readbacks also serve the takeover capture and cancel scenarios."
)]
#[path = "board_e2e/takeover_readbacks.rs"]
mod takeover_readbacks;
use codex_router_test_support::FixtureResult;
use fixture::scenario;
use serde_json::json;

#[test]
fn listener_identity_accepts_matching_shared_debug_port_number() {
    let url = "http://127.0.0.1:43127/mcp";
    assert!(owned_host::require_listener_identity(url, url).is_ok());
}

#[test]
fn listener_identity_accepts_matching_ordinary_port() {
    let url = "http://127.0.0.1:46171/mcp";
    assert!(owned_host::require_listener_identity(url, url).is_ok());
}

#[test]
fn listener_identity_rejects_different_manifest() {
    assert!(
        owned_host::require_listener_identity(
            "http://127.0.0.1:46171/mcp",
            "http://127.0.0.1:46172/mcp"
        )
        .is_err()
    );
}

#[test]
fn listener_identity_rejects_nonloopback_https_implicit_port_and_legacy_ports() {
    for url in [
        "http://192.0.2.1:46171/mcp",
        "https://127.0.0.1:46171/mcp",
        "http://127.0.0.1/mcp",
        "http://127.0.0.1:8787/mcp",
        "http://127.0.0.1:8788/mcp",
    ] {
        assert!(
            owned_host::require_listener_identity(url, url).is_err(),
            "{url}"
        );
    }
}

#[tokio::test]
#[ignore = "Opt-in owned listener counterexample; requires unused 127.0.0.1:43127 and prebuilt board-debug-runtime"]
async fn real_owned_listener_on_43127_matches_manifest_and_serves_installation() -> FixtureResult {
    use codex_router_test_support::scenario_finalization::run_scenario;
    use std::{os::unix::fs::PermissionsExt, path::PathBuf};

    match tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::net::TcpStream::connect(("127.0.0.1", 43127)),
    )
    .await
    {
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {}
        Ok(Ok(_)) => return Err("listener fixture blocked: port 43127 is occupied".into()),
        Ok(Err(error)) => return Err(format!("listener fixture preflight blocked: {error}").into()),
        Err(error) => return Err(format!("listener fixture preflight timed out: {error}").into()),
    }

    // A preflight free-port check is not a reservation. Actual bind failure is
    // a blocked probe, never authority to displace another listener or retry.
    let parent = std::env::var_os("CODEX_ROUTER_E2E_PARENT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = tempfile::Builder::new()
        .prefix("2026-10-board-listener-43127-")
        .tempdir_in(parent)?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let host = match owned_host::BoardHost::start_on_port(root.path(), None, 43127).await {
        Ok(host) => host,
        Err(error) => {
            eprintln!(
                "Rust board listener failure evidence retained: {}",
                root.keep().display()
            );
            return Err(error);
        }
    };
    run_scenario(
        host,
        async |host: &mut owned_host::BoardHost| {
            assert_eq!(reqwest::Url::parse(&host.mcp_url)?.port(), Some(43127));
            let _: collaboration_client::protocol::InstallationIdentity =
                serde_json::from_value(host.mcp("installation_show", json!({})).await?)?;
            Ok(())
        },
        owned_host::BoardHost::stop,
        || Ok(()),
    )
    .await
}

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
