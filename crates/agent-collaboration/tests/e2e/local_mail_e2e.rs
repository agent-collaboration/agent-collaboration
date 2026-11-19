//! LM02–LM03: actual CLI/MCP/Host and authenticated Rust peer, without agent turns.

#![deny(clippy::too_many_lines)]
#[path = "../board_e2e/mail_fixture.rs"]
mod mail_fixture;
#[path = "../board_e2e/mail_messages.rs"]
mod mail_messages;
use crate::owned_host;
use codex_router_test_support::FixtureResult;

#[tokio::test]
async fn lm02_fixed_identity_replays_cli_mcp_once_and_releases_terminal_custody() -> FixtureResult {
    mail_fixture::scenario(mail_messages::fixed_identity).await
}

#[tokio::test]
async fn lm03_lost_response_transport_replays_the_same_identity_without_reinjection()
-> FixtureResult {
    mail_fixture::scenario(mail_messages::lost_answers).await
}
