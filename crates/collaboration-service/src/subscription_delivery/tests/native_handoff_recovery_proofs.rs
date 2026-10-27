//! A committed handoff is the recovery authority when no native custody exists.
#![deny(clippy::too_many_lines)]
use super::native_intent_publisher_proofs::{create_addressed_intent, frozen_item};
use super::*;
use durable_steps::{AtomicStore, CallFailure, StoreError};
use fleet_sync_protocol::DeliveryItem;
use fleet_sync_tokio_runner::{CallEnvelope, CallRouter, TokioHost};

#[tokio::test]
async fn committed_handoff_recovers_the_same_item_after_reopen_before_outbox_admission() {
    let mut fixture = OwnerFixture::new().await;
    let intent = create_addressed_intent(&fixture).await;
    let item =
        fleet_sync_protocol::ItemId::try_from("019f0000-0000-7000-8000-000000041081".to_owned())
            .unwrap();
    let session = target_session(&fixture.reader).unwrap();
    fixture
        .admission
        .claim_intent(
            &fixture.project,
            &intent.intent_id,
            Some(crate::IntentHandoffInput {
                item,
                enrollment: crate::IntentRecipientEnrollment {
                    service: String::from(session.endpoint.service_id)
                        .try_into()
                        .unwrap(),
                    peer: PeerId::try_from("0192a1b2-c3d4-7000-8000-000000000001".to_owned())
                        .unwrap(),
                },
            }),
        )
        .await
        .unwrap();
    let original = frozen_item(&fixture, &intent).await;
    assert_eq!(original.id(), item);
    let push: collaboration_protocol::PushId = item.as_uuid().to_string().try_into().unwrap();
    assert!(
        fixture
            .interactions
            .get_push_record(&push)
            .await
            .unwrap()
            .is_none()
    );

    // The handoff transaction completed; drop and reopen the actual local owner
    // before constructing any publisher or admitting this item to native custody.
    fixture = fixture.reopen_messages().await;
    let runtime = fixture.runtime_without_board().await;
    let (_app, native) = native_application(
        &fixture,
        &runtime,
        PeerId::try_from("0192a1b2-c3d4-7000-8000-000000000001".to_owned()).unwrap(),
    )
    .await;
    let publisher = crate::NotificationPublisher::new(
        fixture.admission.clone(),
        fixture.interactions.clone(),
        Arc::downgrade(&native.runtime_handle()),
    );
    publisher
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .unwrap();
    assert_eq!(request.payload.push_id, push);
    assert_eq!(
        serde_json::to_value(frozen_item(&fixture, &intent).await).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_recovered_custody(&native, &original).await;
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    await_native_completion(&runtime, 1).await;
    publisher
        .queue_project(fixture.project.clone())
        .await
        .unwrap();
    publisher.wait_project_idle_for_test(&fixture.project).await;
    assert!(runtime.requests.lock().await.try_recv().is_err());
    assert_eq!(
        runtime
            .completed_native_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    let record = fixture
        .interactions
        .get_push_record(&push)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.delivery_state, PushDeliveryState::Delivered);
    publisher.shutdown().await;
    drop(publisher);
    drop(_app);
    runtime.close().await;
    fixture.close_native(native).await;
}

struct NoRecoveryObservationCalls;
impl CallRouter for NoRecoveryObservationCalls {
    async fn dispatch(&self, _: CallEnvelope) -> Result<serde_json::Value, CallFailure> {
        panic!("recovery custody observation must not execute a call");
    }
}

async fn assert_recovered_custody(
    native: &NativeMailOwner,
    original: &DeliveryItem<collaboration_contracts::RouterDeliveryTypes>,
) {
    let observer = interaction_storage::InteractionAtomicStore::new(
        native.interactions.clone(),
        Arc::new(TokioHost::new(
            Arc::new(NoRecoveryObservationCalls),
            runtime_primitives::SystemClock,
        )),
    );
    let rows = observer
        .transaction(|scope| {
            Box::pin(async move {
                fleet_sync_delivery::test_support::snapshot(scope)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    let inbox: Vec<_> = rows.iter().filter(|row| row.category == "inbox").collect();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].identity, original.id().as_uuid().to_string());
    let header: serde_json::Value =
        serde_json::from_str(inbox[0].content.as_deref().unwrap()).unwrap();
    // The SDK digest binds every immutable input field, including the body.
    assert_eq!(
        header["digest"],
        serde_json::to_value(original.digest()).unwrap(),
        "recovered native custody changed the pre-reopen frozen input"
    );
}
