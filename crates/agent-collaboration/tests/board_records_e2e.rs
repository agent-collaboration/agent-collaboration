//! VD §0/OB1 and §5a board contracts through real Host/CLI/MCP listeners.
#![allow(clippy::panic_in_result_fn)]
#![deny(clippy::too_many_lines)]
#[path = "board_e2e/complete_pages.rs"]
mod complete_pages;
#[path = "board_e2e/fixture.rs"]
mod fixture;
#[path = "board_e2e/fixture_inputs.rs"]
mod fixture_inputs;
#[path = "board_e2e/owned_host.rs"]
mod owned_host;
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
#[path = "board_e2e/role_checks.rs"]
mod role_checks;
#[path = "board_e2e/scope_inbox.rs"]
mod scope_inbox;
use codex_router_test_support::FixtureResult;
use fixture::scenario;
use serde_json::json;

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
