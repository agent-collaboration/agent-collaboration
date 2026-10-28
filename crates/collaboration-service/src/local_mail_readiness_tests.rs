//! Real SDK readiness release cannot turn a later Auto attempt into a wake.
use super::*;

struct ReadinessPresence(std::sync::atomic::AtomicBool);
impl collaboration_contracts::TargetPresenceProbe for ReadinessPresence {
    fn presence(
        &self,
        _: &collaboration_protocol::SessionRef,
    ) -> collaboration_contracts::DeliveryFuture<'_, collaboration_contracts::TargetPresence> {
        Box::pin(async {
            Ok(if self.0.load(std::sync::atomic::Ordering::SeqCst) {
                collaboration_contracts::TargetPresence::Running
            } else {
                collaboration_contracts::TargetPresence::Wakeable
            })
        })
    }
}

#[tokio::test]
async fn readiness_release_then_unload_does_not_load_or_submit_a_held_auto_again() {
    // Arrange: the real runtime's first native attempt returns a definite-unsent hold.
    let runtime = Runtime::new().await;
    let presence = Arc::new(ReadinessPresence(std::sync::atomic::AtomicBool::new(true)));
    let mut props = runtime.local_mail_props();
    props.presence = presence.clone();
    let mail =
        MailDeliveryRuntime::new(runtime.fixture.host.config.local.owner, props, None).unwrap();
    let draft = runtime.fixture.draft();
    let item = runtime.fixture.item(&draft).id();
    runtime.client.script(
        item,
        vec![
            DeliveryReceipt {
                outcome: DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    reason: collaboration_contracts::NOT_LOADED_REASON.to_owned(),
                },
                reachability: None,
                client: None,
            },
            accepted(),
        ],
    );
    let first = mail
        .send_local(draft.clone(), "readiness fixture".into())
        .await
        .unwrap();
    assert_eq!(first.delivery_state, PushDeliveryState::Held);
    assert_eq!(runtime.client.calls(item), 1);
    // Act: Running authorizes the actual readiness-release transaction. Its committed
    // return is the barrier: custody has its next attempt, while the projection is Pending.
    assert!(matches!(
        presence.presence(&draft.target).await.unwrap(),
        TargetPresence::Running
    ));
    mail.release_held_mail(vec![draft.push_id.clone()])
        .await
        .unwrap();
    assert_eq!(
        runtime
            .fixture
            .owner
            .get_push_record(&draft.push_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Pending
    );
    assert_eq!(
        mail.local_outcome_for_test(item).await.unwrap(),
        Some(RecipientOutcome::Held)
    );
    presence.0.store(false, std::sync::atomic::Ordering::SeqCst);
    mail.drive_due_for_test().await.unwrap();
    // Assert: the recycled Pending row must not grant a second native wake or submission.
    assert_eq!(
        runtime
            .fixture
            .owner
            .get_push_record(&draft.push_id)
            .await
            .unwrap()
            .unwrap()
            .delivery_state,
        PushDeliveryState::Held
    );
    assert_eq!(runtime.client.calls(item), 1);
    drop(mail);
    runtime.close().await;
}
