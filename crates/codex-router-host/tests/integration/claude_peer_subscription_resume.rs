//! A subscription Batch for a Claude Code peer that isn't running is refused at once,
//! whatever its idle policy: nothing will start the terminal or say it started. The posts
//! stay unread on the board, and once the peer runs again a later post reaches it.

#[path = "../support/claude_peer_subscription_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod fixture;

use collaboration_contracts::TargetPresence;
use collaboration_protocol::{PushDeliveryState, PushKind, RouterLink};
use fixture::{ClaudePeerSubscriptionFixture, PRESENCE_RECHECK, ProofResult, SESSION_ID};
use message_board::{InboxActivity, SubscriptionDeliveryOutcome};
use std::{os::unix::fs::PermissionsExt as _, sync::Arc};

const REFUSED: &str = "target is not running; follow batch refused";

#[tokio::test]
async fn claude_peer_subscription_notice_is_refused_while_the_peer_is_closed_and_a_later_post_is_delivered()
 {
    let proof: ProofResult = Box::pin(async {
        // Central's grant is fixture-arranged; this case proves peer delivery, not enrollment.
        let fixture = ClaudePeerSubscriptionFixture::start().await?;
        std::fs::set_permissions(
            fixture.socket_path().parent().expect("registry directory"),
            std::fs::Permissions::from_mode(0o700),
        )?;
        assert_eq!(
            fixture.lookup_peer()?,
            claude_code_peer_messaging::PeerSessionLookup::Absent,
            "the initial session has no published writable peer"
        );
        assert_eq!(fixture.presence().await?, TargetPresence::NotRunning);
        let initial_inbox = fixture.initialize_unread_inbox().await?;
        assert!(initial_inbox.records.is_empty());

        // The peer is closed: the Batch is refused at once and nothing is held.
        let refused_post = fixture.post_activity().await?;
        let refused = (async {
            loop {
                let subscription = fixture.subscription().await?;
                if matches!(&subscription.last_outcome,
                    Some(SubscriptionDeliveryOutcome::NotSubmitted { retryable: false, reason }) if reason == REFUSED)
                {
                    break Ok::<_, Box<dyn std::error::Error + Send + Sync>>(subscription);
                }
                tokio::task::yield_now().await;
            }
        }).await?;
        assert_eq!(refused.held_since, None, "nothing is held");
        assert_eq!(refused.next_retry_at, None, "and nothing is retried");
        assert!(fixture.held_push_records().await?.is_empty());
        assert!(fixture.pending_push_records().await?.is_empty());
        assert!(fixture.due_subscription_roots().await?.is_empty());

        // No timer brings it back: the old presence interval passes with nothing owed.
        fixture.advance(PRESENCE_RECHECK)?;
        fixture.reconcile_reader().await?;
        assert!(fixture.held_push_records().await?.is_empty());
        assert!(fixture.due_subscription_roots().await?.is_empty());

        // The peer runs again; the next post reaches it normally.
        let (listener, writable_peer) = fixture.publish_writable_peer()?;
        assert_eq!(String::from(writable_peer.session_id.clone()), SESSION_ID);
        assert_eq!(fixture.presence().await?, TargetPresence::Running);
        let peer_receiver =
            ClaudePeerSubscriptionFixture::receive_one_peer_batch(Arc::clone(&listener));
        let delivered_post = fixture.post_activity().await?;
        let frames = (peer_receiver).await??;
        assert_eq!(frames.authentication["token"], fixture::PEER_TOKEN);
        assert!(frames.extra_frame.is_none(), "one connection carries one Batch frame");
        let notice_line = frames.user["message"]["content"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("peer frame omitted the notice line"))?;
        assert!(notice_line.starts_with("🧵 Router: new thread activity @"));
        let link_text = notice_line
            .rsplit_once(" · ")
            .map(|(_, link)| link)
            .ok_or_else(|| std::io::Error::other("notice line omitted its Router link"))?;
        let push_id = RouterLink::parse(link_text)?.push_id().clone();
        let delivered = fixture
            .await_push_state(&push_id, PushDeliveryState::Delivered)
            .await?;
        assert_eq!(delivered.kind, PushKind::SubscriptionActivity);
        assert_eq!(delivered.target, *fixture.target());

        // Both posts stay unread on the board: the refusal lost no content.
        let unread_inbox = fixture.unread_project_inbox().await?;
        let unread_posts = unread_inbox
            .records
            .iter()
            .filter_map(|record| {
                if let InboxActivity::PostCreated { post, .. } = record {
                    Some(post.post_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert!(unread_posts.contains(&refused_post.post_id));
        assert!(unread_posts.contains(&delivered_post.post_id));
        assert_eq!(
            fixture.subscription().await?.last_outcome.as_ref(),
            Some(&SubscriptionDeliveryOutcome::Accepted)
        );
        fixture.shutdown().await?;
        Ok(())
    })
    .await;

    proof.expect("Claude peer subscription refusal proof");
}
