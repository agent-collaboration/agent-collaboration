//! Renewal and expiry use the same native project lock and retain one causal end.
use super::*;
use crate::project_admission::MembershipExpiryProposal;
use message_board::{
    BoardOperation, EndReason, HandoffSeq, RecordedActor, RouterComponent, SubscriptionMembership,
    SubscriptionScope,
};
use message_board_storage::{MailCancelGroupCause, MailHandoffItem};

async fn membership(fixture: &OwnerFixture) -> SubscriptionMembership {
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let member = read
        .membership(
            &fixture.reader,
            &SubscriptionScope::thread(fixture.root.clone()),
        )
        .await
        .unwrap()
        .unwrap();
    read.finish().await.unwrap();
    member
}
fn proposal(member: &SubscriptionMembership) -> MembershipExpiryProposal {
    MembershipExpiryProposal {
        membership: member.envelope.id.clone(),
        activated_at: member.activated_at(),
        generation: member.generation(),
        expires_at: member.expires_at(),
    }
}
async fn expiry_groups(fixture: &OwnerFixture, member: &SubscriptionMembership) -> usize {
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let groups = read.mail_handoff_page(HandoffSeq::ZERO, 100).await.unwrap().into_iter().filter(|handoff| matches!(&handoff.item,
        MailHandoffItem::CancelGroup(group) if group.membership.as_str() == member.envelope.id.as_str()
            && group.activated_at == member.activated_at() && group.generation == member.generation()
            && group.cause == MailCancelGroupCause::MembershipEnded { reason: EndReason::Expired }
    )).count();
    read.finish().await.unwrap();
    groups
}

#[tokio::test]
async fn expiry_admission_writes_one_router_end_and_is_idempotent() {
    let fixture = OwnerFixture::new().await;
    let original = membership(&fixture).await;
    assert!(
        fixture
            .admission
            .expire_follow_membership(&fixture.project, proposal(&original), original.expires_at())
            .await
            .unwrap()
    );
    let ended = membership(&fixture).await;
    assert_eq!(ended.reader(), original.reader());
    assert_eq!(ended.ended().unwrap().reason, EndReason::Expired);
    assert!(matches!(
        ended.envelope.actor,
        RecordedActor::Router {
            component: RouterComponent::SubscriptionDelivery,
            ..
        }
    ));
    assert!(
        !fixture
            .admission
            .expire_follow_membership(&fixture.project, proposal(&original), original.expires_at())
            .await
            .unwrap()
    );
    assert_eq!(expiry_groups(&fixture, &original).await, 1);
}

#[tokio::test]
async fn expiry_admission_rechecks_a_membership_renewed_after_the_proposal_was_read() {
    let fixture = OwnerFixture::new().await;
    let original = membership(&fixture).await;
    let stale = proposal(&original);
    fixture
        .write_as(
            fixture.reader.clone(),
            BoardOperation::ThreadRenew {
                scope: SubscriptionScope::thread(fixture.root.clone()),
            },
            original.expires_at() - chrono::Duration::seconds(1),
        )
        .await;
    let result = fixture
        .admission
        .expire_follow_membership(&fixture.project, stale, original.expires_at())
        .await
        .unwrap();
    assert!(
        !result,
        "expiry admission skipped the locked renewal re-check"
    );
    let current = membership(&fixture).await;
    assert!(current.ended().is_none());
    assert!(current.expires_at() > original.expires_at());
    assert_eq!(expiry_groups(&fixture, &original).await, 0);
}

#[tokio::test]
async fn concurrent_renewal_and_expiry_commit_one_of_the_two_serialized_states() {
    for renew_polled_first in [true, false] {
        let fixture = OwnerFixture::new().await;
        let original = membership(&fixture).await;
        let renewal = fixture.write_as(
            fixture.reader.clone(),
            BoardOperation::ThreadRenew {
                scope: SubscriptionScope::thread(fixture.root.clone()),
            },
            original.expires_at() - chrono::Duration::seconds(1),
        );
        let expiry = fixture.admission.expire_follow_membership(
            &fixture.project,
            proposal(&original),
            original.expires_at(),
        );
        let expired = if renew_polled_first {
            let (_, expired) = tokio::join!(renewal, expiry);
            expired.unwrap()
        } else {
            let (expired, _) = tokio::join!(expiry, renewal);
            expired.unwrap()
        };
        let current = membership(&fixture).await;
        assert_eq!(current.ended().is_some(), expired);
        assert_eq!(
            expiry_groups(&fixture, &original).await,
            usize::from(expired)
        );
        if expired {
            assert_eq!(current.ended().unwrap().reason, EndReason::Expired);
            assert_eq!(current.renewed_at(), original.renewed_at());
        } else {
            assert!(current.expires_at() > original.expires_at());
            assert!(current.ended().is_none());
        }
    }
}
