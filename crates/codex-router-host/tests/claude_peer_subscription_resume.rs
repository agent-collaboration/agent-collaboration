#![allow(clippy::expect_used, clippy::indexing_slicing)]
//! A held subscription Batch reaches its real Claude peer once the peer returns.

#[path = "support/claude_peer_subscription_fixture.rs"]
mod fixture;

use collaboration_contracts::TargetPresence;
use collaboration_protocol::{
    PushDeliveryState, PushHeaderFacts, PushKind, RouterLink, RouterOriginRef,
};
use fixture::{
    ClaudePeerSubscriptionFixture, OWNER_EVENT_TIMEOUT, POSTED_MESSAGE_TEXT, PRESENCE_RECHECK,
    ProofResult, ROOT_THREAD_TEXT, SESSION_ID,
};
use message_board::{InboxActivity, SubscriptionDeliveryOutcome};
use std::{os::unix::fs::PermissionsExt as _, sync::Arc};
use tokio::net::UnixListener;

#[tokio::test]
async fn claude_peer_subscription_notice_is_held_then_delivered_once_after_peer_reappears() {
    let proof: ProofResult = async {
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
        let initial_inbox = fixture.initialize_unread_inbox().await?;
        assert!(
            initial_inbox.records.is_empty(),
            "the reader inbox baseline is established before the activity arrives"
        );

        let posted_message = fixture.post_activity().await?;
        let message_board::RecordPlacement::Project(posted_position) = posted_message.position
        else {
            return Err(std::io::Error::other("posted activity is not project-qualified").into());
        };
        let posted_position = posted_position.cursor;
        let held = tokio::time::timeout(OWNER_EVENT_TIMEOUT, async {
            loop {
                let records = fixture.held_push_records().await?;
                if !records.is_empty() { break Ok::<_, Box<dyn std::error::Error + Send + Sync>>(records); }
                tokio::task::yield_now().await;
            }
        }).await.map_err(|_| std::io::Error::other("actual SDK Held item did not appear"))??;
        let held_subscription = fixture.subscription().await?;
        assert_eq!(held_subscription.scope, message_board::SubscriptionScope::thread(fixture.thread_id().clone()));
        assert_eq!(held_subscription.pending_count, 1);
        assert!(held_subscription.held_since.is_some());
        assert_eq!(held_subscription.next_retry_at, None);
        assert!(matches!(held_subscription.last_outcome,
            Some(SubscriptionDeliveryOutcome::NotSubmitted { retryable: true, ref reason }) if reason == "target is not running; follow batch held"));
        assert_eq!(held.len(), 1, "closed Claude peer retains the SDK-owned frozen item");
        let held_notice = &held[0];
        assert_eq!(held_notice.delivery_state, PushDeliveryState::Held);
        assert_eq!(held_notice.kind, PushKind::SubscriptionActivity);
        assert_eq!(held_notice.target, *fixture.target());
        let frozen_activity = held_notice.activity.clone().expect("frozen activity");
        assert_eq!(frozen_activity.ranges.len(), 1);
        assert_eq!(frozen_activity.ranges[0].from, posted_position);
        assert_eq!(frozen_activity.ranges[0].through, posted_position);

        let (listener, writable_peer) = fixture.publish_writable_peer()?;
        assert_eq!(String::from(writable_peer.session_id.clone()), SESSION_ID);
        assert_eq!(writable_peer.socket_path, fixture.socket_path());
        assert_eq!(
            fixture.presence().await?,
            TargetPresence::Running,
            "the real route observes the authenticated writable peer registry record"
        );
        let peer_receiver =
            ClaudePeerSubscriptionFixture::receive_one_peer_batch(Arc::clone(&listener));

        fixture.advance(PRESENCE_RECHECK)?;
        let frames = tokio::time::timeout(OWNER_EVENT_TIMEOUT, peer_receiver)
            .await
            .map_err(|_| std::io::Error::other("peer frame task did not finish"))???;
        assert_eq!(frames.authentication["type"], "auth");
        assert_eq!(frames.authentication["token"], fixture::PEER_TOKEN);
        assert_eq!(frames.user["type"], "user");
        assert_eq!(frames.user["message"]["role"], "user");
        assert!(
            frames.extra_frame.is_none(),
            "one connection must carry one Batch frame"
        );

        let notice_line = frames.user["message"]["content"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("peer frame omitted the notice line"))?;
        assert!(notice_line.starts_with("🧵 Router: new thread activity @"));
        assert!(notice_line.contains("1 thread · 1 message"));
        assert!(!notice_line.contains(POSTED_MESSAGE_TEXT));
        assert!(!notice_line.contains(ROOT_THREAD_TEXT));
        let link_text = notice_line
            .rsplit_once(" · ")
            .map(|(_, link)| link)
            .ok_or_else(|| std::io::Error::other("notice line omitted its Router link"))?;
        let link = RouterLink::parse(link_text)?;
        let push_id = link.push_id().clone();
        assert_eq!(push_id, held_notice.push_id, "the real peer receives the original frozen identity");
        let delivered_notice = fixture.await_push_state(&push_id, PushDeliveryState::Delivered).await?;
        assert_eq!(delivered_notice.activity, Some(frozen_activity));
        assert_eq!(delivered_notice.origin_router_ref, held_notice.origin_router_ref);
        assert_eq!(delivered_notice.kind, PushKind::SubscriptionActivity);
        assert_eq!(
            delivered_notice.delivery_state,
            PushDeliveryState::Delivered
        );
        assert_eq!(delivered_notice.target, *fixture.target());
        assert!(matches!(
            &delivered_notice.header_facts,
            PushHeaderFacts::SubscriptionActivity {
                root_count: 1,
                message_count: 1,
                held_since: Some(_),
                ..
            }
        ));
        let batch_activity = delivered_notice
            .activity
            .as_ref()
            .ok_or_else(|| std::io::Error::other("stored notice omitted Board ranges"))?;
        assert!(batch_activity.held);
        assert!(!batch_activity.draining);
        assert_eq!(batch_activity.ranges.len(), 1);
        assert_eq!(
            batch_activity.ranges[0].project_id,
            posted_message.project_id
        );
        assert_eq!(batch_activity.ranges[0].thread_id, *fixture.thread_id());
        assert_eq!(
            batch_activity.ranges[0].from, posted_position,
            "the stored root range starts at the only posted message"
        );
        assert_eq!(
            batch_activity.ranges[0].through, posted_position,
            "the stored root range ends at the only posted message"
        );

        let batch_reference = RouterOriginRef::parse_canonical(
            delivered_notice
                .origin_router_ref
                .as_deref()
                .ok_or_else(|| {
                    std::io::Error::other("stored notice omitted its batch reference")
                })?,
        )?;
        assert!(matches!(
            &batch_reference,
            RouterOriginRef::SubscriptionActivity {
                target: reference_target,
                ..
            } if reference_target == fixture.target()
        ));
        let only_notice_for_batch = fixture
            .push_record_by_origin_reference(&batch_reference)
            .await?
            .ok_or_else(|| std::io::Error::other("selected Board Batch has no stored notice"))?;
        assert_eq!(only_notice_for_batch.push_id, push_id);
        assert_eq!(
            only_notice_for_batch.origin_router_ref, delivered_notice.origin_router_ref,
            "the peer link resolves to the unique stored Batch identity"
        );

        let unread_inbox = fixture.unread_project_inbox().await?;
        assert_eq!(unread_inbox.records.len(), 1);
        assert!(matches!(
            unread_inbox.records.as_slice(),
            [InboxActivity::PostCreated { post, .. }]
                if post.post_id == posted_message.post_id
        ));
        let settled_subscription = fixture.subscription().await?;
        assert_eq!(settled_subscription.pending_count, 0);
        assert_eq!(
            settled_subscription.last_outcome.as_ref(),
            Some(&SubscriptionDeliveryOutcome::Accepted)
        );
        assert!(fixture.pending_push_records().await?.is_empty());
        assert!(
            fixture.due_subscription_roots().await?.is_empty(),
            "the accepted Batch advances the Board window and leaves no due retry"
        );

        let duplicate_listener: Arc<UnixListener> = Arc::clone(&listener);
        let duplicate_connection = tokio::spawn(async move { duplicate_listener.accept().await });
        fixture.advance(PRESENCE_RECHECK)?;
        fixture.reconcile_reader().await?;
        assert!(
            !duplicate_connection.is_finished(),
            "the real peer listener receives no duplicate Batch during the following owner cycle"
        );
        duplicate_connection.abort();
        let _cancelled_accept = duplicate_connection.await;

        fixture.shutdown().await?;
        Ok(())
    }
    .await;

    proof.expect("Claude peer subscription resume proof");
}
