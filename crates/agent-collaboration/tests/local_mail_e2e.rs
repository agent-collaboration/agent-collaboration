//! LM01–LM05: actual CLI/MCP/Host and authenticated Rust peer, without agent turns.
#![allow(clippy::panic_in_result_fn)]
#![deny(clippy::too_many_lines)]
#[path = "board_e2e/mail_fixture.rs"]
mod mail_fixture;
#[path = "board_e2e/mail_messages.rs"]
mod mail_messages;
#[path = "board_e2e/mail_migration.rs"]
mod mail_migration;
#[path = "board_e2e/mail_wakes.rs"]
mod mail_wakes;
#[path = "board_e2e/owned_host.rs"]
mod owned_host;
use codex_router_test_support::FixtureResult;

#[tokio::test]
async fn lm01_legacy_migration_retains_ids_terminal_answers_and_never_reinjects() -> FixtureResult {
    mail_fixture::scenario(mail_migration::legacy_migration).await
}

#[tokio::test]
async fn lm02_fixed_identity_replays_cli_mcp_once_and_releases_terminal_custody() -> FixtureResult {
    mail_fixture::scenario(mail_messages::fixed_identity).await
}

#[tokio::test]
async fn lm03_lost_response_transport_replays_the_same_identity_without_reinjection()
-> FixtureResult {
    mail_fixture::scenario(mail_messages::lost_answers).await
}

#[tokio::test]
async fn lm04_wake_creator_replacement_replay_and_lifetime_matrix() -> FixtureResult {
    mail_fixture::scenario(mail_wakes::slots_and_limits).await
}

#[tokio::test]
async fn lm05_due_relative_end_boundaries_and_actual_one_shot_peer_delivery() -> FixtureResult {
    mail_fixture::scenario(mail_wakes::due_relative_end).await
}
