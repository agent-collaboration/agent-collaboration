//! CS-2: terminal non-project mail releases central custody through the real Host.
#![allow(dead_code, clippy::panic_in_result_fn)]
#![deny(clippy::too_many_lines)]
#[path = "board_e2e/central_pair.rs"]
mod central_pair;
#[path = "board_e2e/fixture.rs"]
mod fixture;
#[path = "mail_retirement/lost_answer.rs"]
mod lost_answer;
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
async fn ordinary_requested_control_retires_and_keeps_its_result_after_restart() -> FixtureResult {
    central_pair::scenario(wake_and_capacity::requested_control).await
}

#[tokio::test]
async fn terminal_direct_mail_releases_the_only_admission_slot() -> FixtureResult {
    central_pair::scenario_with_small_mailbox(wake_and_capacity::capacity).await
}

#[tokio::test]
async fn remote_wake_requests_and_mutations_retire_their_central_reservations() -> FixtureResult {
    central_pair::scenario(wake_and_capacity::wakes).await
}

#[tokio::test]
async fn lost_retirement_answer_resumes_the_same_call_after_sender_host_restart() -> FixtureResult {
    central_pair::scenario(lost_answer::run).await
}

#[tokio::test]
async fn delivered_direct_mail_is_compacted_and_keeps_terminal_replay_after_host_restart()
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
            readback::await_compacted(fixture, &String::from(service.clone()), &item).await?;
            sender_host.stop_host().await?;
            sender_host.reopen_host().await?;
            assert_eq!(wake_and_capacity::call(sender_host.host()?,"message_send",request).await?, queued);
            let shown=sender_host.host()?.mcp("router_show",json!({"caller":sender,"reference":item})).await?;
            assert_eq!(shown.pointer("/mail/status/kind"),Some(&json!("delivered")),"{shown}");
            assert_eq!(peer.count_matching_after_barrier("retirement direct marker").await?,1);
            readback::require_compacted(fixture, &String::from(service), &item).await
        }, PeerFixture::shutdown, || Ok(())).await
    }).await
}
