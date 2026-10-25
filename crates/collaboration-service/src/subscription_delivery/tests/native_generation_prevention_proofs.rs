//! SDK prevention leaves the frozen range owed under the reader's new generation.
use super::native_follow_mail_proofs::{follow_application, frozen_follow};
use super::*;
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::NotificationSource;

#[tokio::test]
async fn forged_prevention_references_are_refused_before_the_locked_replacement_allocation() {
    use super::follow_admission_proofs::{current_membership, proposal};
    // Arrange an actual prevented original, but stop the reader before it can replace it.
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    let position = fixture
        .post(&fixture.root, "one frozen range for locked validation")
        .await;
    let target = target_session(&fixture.reader).unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        while native
            .interactions
            .list_held_mail_push_records(&target)
            .await
            .unwrap()
            .is_empty()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    runtime.service.shutdown().await;
    fixture
        .policy(
            &fixture.root,
            message_board::SubscriptionMode::Deliver,
            message_board::WhenIdle::Hold,
            1,
            2,
        )
        .await;
    publisher
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    let member = current_membership(&fixture).await;
    let facts = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let facts = publisher
                .generation_preventions(
                    std::slice::from_ref(&fixture.project),
                    std::slice::from_ref(&member),
                )
                .await
                .unwrap();
            if !facts.is_empty() {
                break facts;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(facts.len(), 1);
    let fact = &facts[0];
    let mut other_cancel = fact.cancel().clone();
    other_cancel.item = fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7()).unwrap();
    let mut changed_target = fact.target().clone();
    changed_target.digest = fact.cancel().digest.clone();
    let forged = [
        fact.substituted_for_test(fact.group().clone(), fact.target().clone(), other_cancel),
        fact.substituted_for_test(fact.group().clone(), changed_target, fact.cancel().clone()),
        fact.substituted_for_test(
            uuid::Uuid::now_v7().to_string().try_into().unwrap(),
            fact.target().clone(),
            fact.cancel().clone(),
        ),
    ];
    // Act and assert every mismatch at the real project admission transaction.
    for bad in forged {
        let mut input = proposal(&fixture, &member, position);
        input.preventions = vec![bad];
        assert!(
            matches!(
                fixture
                    .admission
                    .allocate_follow_batch(&fixture.project, input, fixture.clock.now())
                    .await,
                Err(crate::ProjectAdmissionError::InvalidRecord(_))
            ),
            "forged SDK observation bypassed locked reference validation"
        );
        assert_eq!(frozen_follow(&fixture).await.len(), 1);
    }
    let mut correct = proposal(&fixture, &member, position);
    correct.preventions = facts;
    assert!(
        fixture
            .admission
            .allocate_follow_batch(&fixture.project, correct.clone(), fixture.clock.now())
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(frozen_follow(&fixture).await.len(), 2);
    fixture
        .unsubscribe(
            message_board::ThreadSubscriptionUnsubscribeRequest {
                reader: fixture.reader.clone(),
                scope: message_board::SubscriptionScope::thread(fixture.root.clone()),
            },
            fixture.clock.now(),
        )
        .await
        .unwrap();
    correct.item = fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7()).unwrap();
    assert!(
        fixture
            .admission
            .allocate_follow_batch(&fixture.project, correct, fixture.clock.now())
            .await
            .unwrap()
            .is_none(),
        "membership end created new notification debt"
    );
    assert_eq!(frozen_follow(&fixture).await.len(), 2);
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;
}

#[tokio::test]
async fn a_generation_prevented_range_is_replaced_once_and_reaches_the_native_client() {
    // Arrange actual project admission, reader windows and native SDK custody.
    let fixture = OwnerFixture::new().await;
    *fixture.presence.0.lock().unwrap() = TargetPresence::Wakeable;
    let runtime = fixture.runtime().await;
    let (app, native, publisher) = follow_application(&fixture, &runtime).await;
    let position = fixture
        .post(&fixture.root, "still owed after a policy change")
        .await;
    let target = target_session(&fixture.reader).unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        while native
            .interactions
            .list_held_mail_push_records(&target)
            .await
            .unwrap()
            .is_empty()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let original = frozen_follow(&fixture).await;
    assert_eq!(original.len(), 1);

    // Act through the real membership change and publisher's existing hint entry.
    fixture
        .policy(
            &fixture.root,
            message_board::SubscriptionMode::Deliver,
            message_board::WhenIdle::Hold,
            1,
            2,
        )
        .await;
    publisher
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if matches!(
                native
                    .runtime()
                    .local_outcome_for_test(original[0].id())
                    .await
                    .unwrap(),
                Some(fleet_sync_protocol::RecipientOutcome::Prevented { .. })
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fixture.clock.advance_without_tokio_time(2);
    let replacements = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let items = frozen_follow(&fixture).await;
            if items.len() >= 2 {
                break items;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let mut received = None;
    if replacements.is_ok() {
        *fixture.presence.0.lock().unwrap() = TargetPresence::Running;
        fixture.clock.advance_without_tokio_time(30);
        let request = tokio::time::timeout(Duration::from_secs(6), async {
            runtime.requests.lock().await.recv().await.unwrap()
        })
        .await
        .unwrap();
        received = Some(request.payload.push_id);
        runtime.completions.send(DeliveryOutcome::Started).unwrap();
        await_native_completion(&runtime, 1).await;
    }
    publisher.shutdown().await;
    drop(publisher);
    drop(app);
    runtime.close().await;
    fixture.close_native(native).await;

    // Assert literal contract debt, distinct generation and the actual client boundary.
    let items = replacements.expect("generation-prevented activity was counted as covered");
    assert_eq!(items.len(), 2, "one owed range allocated more than once");
    let mut generations = Vec::new();
    for item in &items {
        let BoardDeliveryBody::Notification(body) = item.body() else {
            panic!("notification");
        };
        let NotificationSource::FollowBatch {
            generation, roots, ..
        } = &body.source
        else {
            panic!("follow");
        };
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].from(), position);
        assert_eq!(roots[0].through(), position);
        assert_eq!(roots[0].post_count(), 1);
        generations.push(*generation);
    }
    assert_ne!(generations[0], generations[1]);
    assert_eq!(
        received.unwrap().as_str(),
        items[1].id().as_uuid().to_string()
    );
}
