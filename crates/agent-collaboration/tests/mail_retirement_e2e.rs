//! Terminal non-project mail survives replay and restart without sender retirement.

#![deny(clippy::too_many_lines)]
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/central_pair.rs"]
mod central_pair;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/fixture.rs"]
mod fixture;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/fixture_inputs.rs"]
mod fixture_inputs;
#[expect(
    dead_code,
    reason = "Shared native observer serves several integration targets."
)]
#[path = "board_e2e/messaging_stopped_owner.rs"]
mod messaging_stopped_owner;
#[expect(
    dead_code,
    reason = "Shared native observer serves several integration targets."
)]
#[path = "support/local_mail_native_sql.rs"]
mod native_sql;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "board_e2e/owned_host.rs"]
mod owned_host;
#[path = "mail_retirement/readback.rs"]
mod readback;
#[path = "mail_retirement/wake_and_capacity.rs"]
mod wake_and_capacity;
use codex_router_test_support::{
    FixtureResult, owned_peer_fixture::PeerFixture, scenario_finalization::run_scenario,
};
use collaboration_client::protocol::{EndpointRef, SessionRef};
use serde_json::json;

#[tokio::test]
async fn terminal_direct_mail_retains_custody_while_later_items_deliver() -> FixtureResult {
    central_pair::scenario(wake_and_capacity::capacity).await
}

#[tokio::test]
async fn remote_wake_requests_and_mutations_keep_terminal_custody() -> FixtureResult {
    central_pair::scenario(wake_and_capacity::wakes).await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn delivered_direct_mail_keeps_terminal_custody_and_replay_after_host_restart()
-> FixtureResult {
    central_pair::scenario(async |fixture, sender_host| {
        let service = fixture.installation.as_ref().ok_or("recipient installation absent")?.service_id.clone();
        let endpoint: EndpointRef = serde_json::from_value(json!({"serviceId":service,"endpointId":"claude-local"}))?;
        let peer = wake_and_capacity::start_peer(fixture, &endpoint).await?;
        run_scenario(peer, async |peer: &mut PeerFixture| {
            let sender: SessionRef = serde_json::from_value(json!({"endpoint":{"serviceId":sender_host.service_id()?,"endpointId":"codex-local"},"sessionId":"retirement-sender"}))?;
            let item = fixture::identity();
            let request = json!({"itemId":item,"target":peer.target,"message":{"kind":"agent","sender":sender,"text":"retirement direct marker"},"delivery":"auto"});
            let queued = wake_and_capacity::call(sender_host.host()?,"message_send",request.clone()).await?;
            peer.expect_marker("retirement direct marker").await?;
            readback::require_delivered(sender_host.host()?, &serde_json::to_value(&sender)?, &item).await?;
            sender_host.stop_host().await?;
            sender_host.reopen_host().await?;
            assert_eq!(wake_and_capacity::call(sender_host.host()?,"message_send",request).await?, queued);
            let shown=sender_host.host()?.mcp("router_show",json!({"caller":sender,"reference":item})).await?;
            assert_eq!(shown.pointer("/mail/status/kind"),Some(&json!("delivered")),"{shown}");
            assert_eq!(peer.count_matching_after_barrier("retirement direct marker").await?,1);
            let stopped = sender_host.stop_for_readback().await?;
            readback::require_cold_terminal(&stopped, &item).await
        }, PeerFixture::shutdown, || Ok(())).await
    }).await
}
