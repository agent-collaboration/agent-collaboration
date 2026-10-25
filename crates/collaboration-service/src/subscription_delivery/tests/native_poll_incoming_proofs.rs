//! Remote receive obeys frozen Poll policy without a local sender project or membership.
use super::incoming_poll_session::IncomingPollSession;
use super::native_follow_mail_proofs::follow_application_with_session;
use super::*;
use crate::{SubscriptionWaitFilter, SubscriptionWaitResult};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::{
    MailSender, NotificationBody, NotificationSource, RecordPointer, RootRange,
};
use fleet_sync_protocol::{DeliveryItem, ItemId, Recipient, RecipientOutcome};
use message_board::*;

#[tokio::test]
async fn remote_incoming_poll_uses_its_frozen_policy_without_opening_the_senders_project() {
    let fixture = OwnerFixture::new().await;
    let local = super::follow_admission_proofs::current_membership(&fixture).await;
    assert_eq!(local.policy().mode(), SubscriptionMode::Deliver);
    let remote_project = ProjectId::generate();
    let root = ThreadId::generate();
    let at = FeedCursor {
        epoch: EpochNumber::INITIAL,
        position: FeedPosition::try_from(2).unwrap(),
    };
    let from = FeedCursor {
        epoch: EpochNumber::INITIAL,
        position: FeedPosition::try_from(1).unwrap(),
    };
    let policy = SubscriptionPolicy::defaults_for(&fixture.reader)
        .apply_patch(
            &fixture.reader,
            &SubscriptionPolicyPatch {
                mode: Some(SubscriptionMode::Poll),
                when_idle: None,
                timing: SubscriptionTimingPatch {
                    quiet_seconds: Some(0),
                    cap_seconds: Some(0),
                },
                lifetime_seconds: None,
            },
        )
        .unwrap();
    let peer = PeerId::try_from("0192a1b2-c3d4-7000-8000-000000000001".to_owned()).unwrap();
    let Identity::Session { session: target } = &fixture.reader else {
        panic!("session recipient");
    };
    let item = DeliveryItem::<BoardDeliveryTypes>::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        MailSender::Project {
            project: remote_project.clone(),
        },
        Recipient {
            peer,
            address: target.clone(),
        },
        None,
        BoardDeliveryBody::Notification(NotificationBody {
            excerpt: "remote frozen Poll".to_owned().try_into().unwrap(),
            pointer: RecordPointer {
                project: remote_project.clone(),
                subject: root.clone().into(),
                position: at,
            },
            source: NotificationSource::FollowBatch {
                membership: RecordId::generate().as_str().to_owned().try_into().unwrap(),
                activated_at: FeedCursor {
                    epoch: EpochNumber::INITIAL,
                    position: FeedPosition::ZERO,
                },
                generation: local.generation(),
                roots: vec![RootRange::new(root.clone(), from, at, 2).unwrap()],
                policy,
                draining: false,
            },
        }),
    )
    .unwrap();
    let id = item.id();
    let push: collaboration_protocol::PushId = id.as_uuid().to_string().try_into().unwrap();
    let transport = IncomingPollSession::open(fixture.directory.path(), peer, item).await;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) =
        follow_application_with_session(&fixture, &runtime, Some(transport.session.clone())).await;
    assert!(
        fixture
            .registry
            .existing_project(&remote_project)
            .await
            .unwrap()
            .is_none()
    );
    native.runtime().prompt_receive_all().await.unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        while !transport
            .reports
            .lock()
            .unwrap()
            .iter()
            .any(|(item, outcome)| *item == id && matches!(outcome, RecipientOutcome::Held))
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("remote SDK receive did not report its Poll readiness hold");
    let before = fixture.interactions.pending_poll_item(id).await.unwrap();
    let no_push = fixture
        .interactions
        .get_push_record(&push)
        .await
        .unwrap()
        .is_none();
    let result = tokio::time::timeout(
        Duration::from_secs(6),
        runtime.service.wait(
            fixture.reader.clone(),
            SubscriptionWaitFilter::All,
            60,
            usize::MAX,
        ),
    )
    .await;
    if matches!(&result, Ok(Ok(Some(_)))) {
        fixture
            .await_push_state(&push, PushDeliveryState::Delivered)
            .await;
        tokio::time::timeout(Duration::from_secs(6), async {
            while !transport
                .reports
                .lock()
                .unwrap()
                .iter()
                .any(|(item, outcome)| {
                    *item == id && matches!(outcome, RecipientOutcome::Delivered { .. })
                })
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the SDK did not send its Poll Started report over the session");
    }
    let after = fixture.interactions.pending_poll_item(id).await.unwrap();
    let opened_sender = fixture
        .registry
        .existing_project(&remote_project)
        .await
        .unwrap()
        .is_some();
    let native_called = runtime.requests.lock().await.try_recv().is_ok();
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    let directory = fixture.into_directory();
    native.close().await.unwrap();
    transport.close().await;
    drop(directory);
    assert!(before.is_some() && no_push);
    let SubscriptionWaitResult::Notice { push_id, batch, .. } = result
        .expect("remote Poll item was not discovered by the waiter")
        .unwrap()
        .unwrap()
    else {
        panic!("session notice");
    };
    assert_eq!(push_id, push);
    assert_eq!(batch.roots.len(), 1);
    let received = batch.roots.first().unwrap();
    assert_eq!(received.thread_id, root);
    assert_eq!(received.project_id, remote_project);
    assert_eq!(received.post_count, 2);
    assert!(after.is_none());
    assert!(!opened_sender);
    assert!(!native_called);
}
