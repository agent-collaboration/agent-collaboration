//! A Claude Code peer that isn't running has its follow mail refused at once, whatever its
//! idle policy: nothing is held, the post stays unread on the board, and once the peer runs
//! again a later post reaches it.
use super::{
    fixture::{BoardFixture, at, identity},
    follow_fixture::{FollowMembership, native_state, only_item},
    notification_peer::NotificationPeer,
    takeover_readbacks::{deliveries, handoffs},
};
use codex_router_test_support::FixtureResult;
use collaboration_client::protocol::{EndpointId, EndpointRef};
use collaboration_protocol::{DeliveryOutcome, PushDeliveryState, SessionRef};
use owned_process_test_support::scenario_finalization::run_scenario;
use serde_json::{Value, json};

const FOLLOW_SUMMARY: &str = "Router: new thread activity";

pub async fn closed_peer_follow_is_refused_and_a_later_post_reaches_it(
    fixture: &mut BoardFixture,
) -> FixtureResult {
    fixture.create().await?;
    let installation = fixture.installation.as_ref().ok_or("installation absent")?;
    let endpoint = EndpointRef {
        service_id: installation.service_id.clone(),
        endpoint_id: EndpointId::try_from("claude-local".to_owned())?,
    };
    let session: collaboration_client::protocol::SessionId = identity().try_into()?;
    let target: SessionRef =
        serde_json::from_value(json!({"endpoint":endpoint,"sessionId":session}))?;
    let reader = json!({"kind":"session","session":target});

    // No terminal is open for the session. Its follow uses whenIdle=hold, and still the
    // batch is refused at once rather than held for a terminal nothing can start.
    let follow = FollowMembership::arrange(fixture, target.clone()).await?;
    let refused_post = fixture
        .reply(
            &fixture.project,
            &follow.thread,
            "posted while the terminal is closed",
        )
        .await?;
    fixture
        .host
        .as_mut()
        .ok_or("refusal publisher Host absent")?
        .drain_project_publisher(&fixture.project)
        .await?;
    let stopped = fixture
        .host
        .take()
        .ok_or("refusal readback Host absent")?
        .stop_for_readback()
        .await?;
    let recipient: message_board::SessionRef =
        serde_json::from_value(serde_json::to_value(&target)?)?;
    let item = only_item(
        deliveries(handoffs(&stopped, &fixture.project, 1).await?)
            .into_iter()
            .filter(|item| item.recipient().address == recipient)
            .collect(),
    )?;
    drop(stopped);
    fixture.reopen_host().await?;
    let refused = native_state(
        fixture.host()?,
        &target,
        &item.id().as_uuid().to_string(),
        PushDeliveryState::Rejected,
    )
    .await?;
    assert!(
        matches!(
            refused.record.last_outcome.as_ref().map(|receipt| &receipt.outcome),
            Some(DeliveryOutcome::NotSubmitted { retryable: false, reason })
                if reason == "target is not running; follow batch refused"
        ),
        "refused at once, not held: {:?}",
        refused.record.last_outcome
    );
    // The refusal lost no content: the post is still unread for the reader.
    let unread = fixture.inbox(&reader, json!({})).await?;
    assert!(
        post_ids(&unread)?.contains(&at(&refused_post, "/postId")?),
        "the refused post stays unread: {unread}"
    );

    // The terminal opens; a later post reaches it as one notice.
    run_scenario(
        None,
        async |peer: &mut Option<NotificationPeer>| {
            *peer = Some(
                NotificationPeer::start(&fixture.directory, endpoint.clone(), session.clone())
                    .await?,
            );
            let peer = peer.as_mut().ok_or("peer absent")?;
            assert_eq!(peer.target, target);
            fixture
                .reply(
                    &fixture.project,
                    &follow.thread,
                    "posted once the terminal is open",
                )
                .await?;
            peer.expect_text(FOLLOW_SUMMARY).await?;
            assert_eq!(peer.count_matching(FOLLOW_SUMMARY).await?, 1);
            Ok(())
        },
        async |peer: Option<NotificationPeer>| match peer {
            Some(peer) => peer.shutdown().await,
            None => Ok(()),
        },
        || Ok(()),
    )
    .await
}

fn post_ids(page: &Value) -> FixtureResult<Vec<&Value>> {
    page.pointer("/page/records")
        .and_then(Value::as_array)
        .ok_or("inbox page absent")?
        .iter()
        .filter_map(|record| record.pointer("/post/postId"))
        .map(Ok)
        .collect()
}
