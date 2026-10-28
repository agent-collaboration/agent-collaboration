//! Real SDK outbox capacity blocks a frozen child; remote outcomes and ACK keys are fixtures.
use super::native_generation_isolation_proofs::arrange_remote_generation_cancel;
use super::*;
use crate::frozen_mail_query_fixture::FrozenMailQuery;
use durable_steps::{AtomicStore, CallFailure};
use fleet_sync_delivery::{
    DeliveryLimits, Outbox, OutboxAdmission, delivery_rollback, minimum_settlement_allowance,
};
use fleet_sync_protocol::{
    CancelResult, DeliveryEnvelope, DeliveryFailureKind, DeliveryItem, DeliveryRefusal,
    DeliveryStatus, ItemId, ItemStatus, ItemStatusState, StatusVersion,
};
use fleet_sync_tokio_runner::{CallEnvelope, CallRouter, TokioHost};
use message_board::{CancelExpansionSeq, EpochNumber, FeedCursor, FeedPosition, ProjectId};
use message_board_storage::{AcknowledgedProjectChanges, MailHandoffItem};

struct NoFillerCalls;
impl CallRouter for NoFillerCalls {
    async fn dispatch(&self, _: CallEnvelope) -> Result<serde_json::Value, CallFailure> {
        panic!("quota filler has custody only and must never execute a call");
    }
}
struct QuotaAcknowledgements(AcknowledgedProjectChanges);
impl collaboration_contracts::ProjectReplicationPosition for QuotaAcknowledgements {
    fn acknowledged_mail_changes<'a>(
        &'a self,
        _: &'a ProjectId,
    ) -> collaboration_contracts::AcknowledgedMailChangesFuture<'a> {
        Box::pin(async { Ok(self.0.clone()) })
    }
    fn replicated_through<'a>(
        &'a self,
        _: &'a ProjectId,
        epoch: EpochNumber,
    ) -> collaboration_contracts::ReplicatedPositionFuture<'a> {
        Box::pin(async move {
            Ok(FeedCursor {
                epoch,
                position: FeedPosition::ZERO,
            })
        })
    }
}

#[tokio::test]
async fn a_full_native_outbox_keeps_its_frozen_child_pending_until_real_capacity_returns() {
    Box::pin(tokio::time::timeout(Duration::from_secs(30), async {
        // Arrange an inherited remote original: no sender row reserves its cancel slot.
        let fixture = Box::pin(OwnerFixture::new()).await;
        let runtime = fixture.runtime().await;
        let mut batches = Vec::new();
        for _ in 0..3 {
            batches.push(arrange_remote_generation_cancel(&fixture).await);
        }
        let (_, target, child) = batches.first().unwrap().clone();
        let memberships: Vec<_> = batches
            .iter()
            .map(|(member, _, _)| member.clone())
            .collect();
        let store = fixture
            .registry
            .existing_project(&fixture.project)
            .await
            .unwrap()
            .unwrap();
        let mut read = store.snapshot().await.unwrap();
        let target_id = target.item.as_uuid().to_string().try_into().unwrap();
        let original = read.mail_handoff(&target_id).await.unwrap().unwrap();
        let original_sequence = original.sequence;
        let MailHandoffItem::Delivery(original) = original.item else {
            panic!("original item")
        };
        let DeliveryEnvelope::Item(original) = *original else {
            panic!("original delivery")
        };
        let children = read
            .mail_cancel_expansion_page(CancelExpansionSeq::ZERO, 100)
            .await
            .unwrap();
        assert_eq!(children.len(), 3);
        let frozen_children: Vec<_> = children
            .iter()
            .map(|row| {
                (
                    row.sequence,
                    row.expansion.group_handoff.clone(),
                    row.expansion.target_item.clone(),
                    serde_json::to_value(&row.expansion.envelope).unwrap(),
                )
            })
            .collect();
        let expansion = &children[0];
        let frozen = serde_json::to_value(&expansion.expansion.envelope).unwrap();
        let child_sequence = children.last().unwrap().sequence;
        let group_sequence = read.mail_handoff_head().await.unwrap();
        let epoch = read.high_water().epoch;
        read.finish().await.unwrap();
        let peer = PeerId::try_from("0192a1b2-c3d4-7000-8000-000000000001".to_owned()).unwrap();
        let query = FrozenMailQuery::open(fixture.directory.path(), peer).await;
        query.accept_submissions();
        query.release();
        let limits = DeliveryLimits::new::<collaboration_contracts::RouterDeliveryTypes>(
            32 * 1024 * 1024,
            100,
            32 * 1024 * 1024,
            1,
            32 * 1024 * 1024,
            minimum_settlement_allowance::<collaboration_contracts::RouterDeliveryTypes>().unwrap(),
        )
        .unwrap();
        let observer = interaction_storage::InteractionAtomicStore::new(
            fixture.interactions.clone(),
            Arc::new(TokioHost::new(
                Arc::new(NoFillerCalls),
                runtime_primitives::SystemClock,
            )),
        );
        let filler = DeliveryItem::<collaboration_contracts::RouterDeliveryTypes>::new(
            ItemId::new(uuid::Uuid::now_v7()).unwrap(),
            original.sender().clone(),
            original.recipient().clone(),
            None,
            serde_json::from_value(serde_json::to_value(original.body()).unwrap()).unwrap(),
        )
        .unwrap();
        let filler_id = filler.id();
        let relay = query.session.relay;
        let queued = observer
            .transaction(move |scope| {
                Box::pin(async move {
                    Outbox::<_, collaboration_contracts::RouterDeliveryTypes>::new(scope, limits)
                        .enqueue(filler, relay)
                        .await
                        .map_err(delivery_rollback)
                })
            })
            .await
            .unwrap();
        assert!(matches!(queued, OutboxAdmission::Queued { .. }));
        let mail = Arc::new(
            crate::MailDeliveryRuntime::with_test_session_limits(
                peer,
                crate::MailPushDeliveryProps {
                    interactions: fixture.interactions.clone(),
                    actors: runtime.service.clone(),
                    presence: fixture.presence.clone(),
                    machine: collaboration_contracts::MachineIdentity::new(
                        SERVICE_ID.to_owned().try_into().unwrap(),
                        None,
                    )
                    .unwrap(),
                    clock: runtime.service.subscription_clock(),
                },
                limits,
                query.session.clone(),
            )
            .unwrap(),
        );
        let marks = Arc::new(QuotaAcknowledgements(AcknowledgedProjectChanges {
            max_handoff_seq: Some(group_sequence.get()),
            max_expansion_seq: Some(child_sequence.get()),
            ..Default::default()
        }));
        let identity = ApplicationFixture::new(SERVICE_ID, SERVICE_ID)
            .unwrap()
            .with_project_replication(marks)
            .with_project_admission(fixture.admission.clone())
            .with_automation_store(fixture.automation.clone())
            .with_subscription_delivery_service(runtime.service.clone(), fixture.presence.clone());
        let (identity, native) =
            NativeMailOwner::start_prepared(identity, peer, fixture.interactions.clone(), mail)
                .unwrap();
        let publisher = Arc::new(crate::NotificationPublisher::new(
            fixture.admission.clone(),
            fixture.interactions.clone(),
            Arc::downgrade(&native.runtime_handle()),
        ));
        let app = Arc::new((identity.with_notification_publisher(publisher.clone())).build());
        let control: fleet_sync_protocol::DeliveryCancel<
            collaboration_contracts::RouterDeliveryTypes,
        > = match serde_json::from_value::<
            DeliveryEnvelope<collaboration_contracts::RouterDeliveryTypes>,
        >(frozen.clone())
        .unwrap()
        {
            DeliveryEnvelope::Cancel(control) => control,
            DeliveryEnvelope::Item(_) => panic!("frozen child control"),
        };
        let refusal = native
            .runtime()
            .enqueue_project_cancel(control, epoch)
            .await;
        assert!(
            matches!(
                refusal,
                Err(fleet_sync_delivery::DeliveryTransactionFailure::Refused(
                    DeliveryRefusal::MailboxFull
                ))
            ),
            "actual native outbox must enforce its one occupied slot"
        );
        let observed = publisher
            .generation_preventions(std::slice::from_ref(&fixture.project), &memberships)
            .await
            .unwrap();
        assert!(
            observed.is_empty(),
            "outbox backpressure was mistaken for terminal child prevention"
        );
        let child_id: collaboration_protocol::MailItemId =
            child.item.as_uuid().to_string().try_into().unwrap();
        assert!(
            fixture
                .interactions
                .mail_submission_epoch(&child_id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(query.submissions().is_empty());
        publisher
            .advance_project_frontier_for_test(&fixture.project)
            .await
            .unwrap();
        assert!(super::native_frontier_proofs::frontier(&fixture).await < original_sequence);

        // Free the real SDK quota; no SQL accounting changes or fake MailboxFull answers.
        observer
            .transaction(move |scope| {
                Box::pin(async move {
                    Outbox::<_, collaboration_contracts::RouterDeliveryTypes>::new(scope, limits)
                        .record_failure(filler_id, DeliveryFailureKind::ExecutionFailed)
                        .await
                        .map_err(delivery_rollback)
                })
            })
            .await
            .unwrap();
        let mut submitted = Vec::new();
        for (member, target, child) in batches {
            publisher
                .generation_preventions(
                    std::slice::from_ref(&fixture.project),
                    std::slice::from_ref(&member),
                )
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(6), async {
                while query.submissions().len() == submitted.len() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("the next frozen child must publish after actual quota release");
            submitted.push(child.clone());
            assert_eq!(
                query.submissions(),
                submitted,
                "quota retry duplicated or reordered frozen children"
            );
            let child_id: collaboration_protocol::MailItemId =
                child.item.as_uuid().to_string().try_into().unwrap();
            let submission = fixture
                .interactions
                .mail_submission_epoch(&child_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(submission.epoch, epoch);
            // Canonical remote status is a fixture input; its real SDK application frees quota.
            for reference in [&child, &target] {
                query.answer_for(
                    reference.item,
                    ItemStatus {
                        item: reference.item,
                        state: ItemStatusState::Known {
                            digest: reference.digest.clone(),
                            status: DeliveryStatus::Cancelled,
                            version: StatusVersion::new(2),
                            cancel: if reference.item == child.item {
                                Some(CancelResult::Prevented)
                            } else {
                                None
                            },
                        },
                    },
                );
            }
            let facts = publisher
                .generation_preventions(
                    std::slice::from_ref(&fixture.project),
                    std::slice::from_ref(&member),
                )
                .await
                .unwrap();
            assert_eq!(facts.len(), 1);
            assert_eq!(facts[0].target(), &target);
            assert_eq!(facts[0].cancel(), &child);
            let item = child.item;
            observer
                .transaction(move |scope| {
                    Box::pin(async move {
                        Outbox::<_, collaboration_contracts::RouterDeliveryTypes>::new(
                            scope, limits,
                        )
                        .publish_status(fleet_sync_protocol::StatusPublication {
                            item,
                            status: DeliveryStatus::Cancelled,
                            version: StatusVersion::new(2),
                            cancel: Some(CancelResult::Prevented),
                        })
                        .await
                        .map_err(delivery_rollback)
                    })
                })
                .await
                .unwrap();
            publisher
                .generation_preventions(
                    std::slice::from_ref(&fixture.project),
                    std::slice::from_ref(&member),
                )
                .await
                .unwrap();
            assert_eq!(query.submissions(), submitted);
        }
        assert_eq!(submitted.len(), 3);
        let facts = publisher
            .generation_preventions(std::slice::from_ref(&fixture.project), &memberships)
            .await
            .unwrap();
        assert_eq!(facts.len(), 3);
        assert_eq!(query.submissions(), submitted);
        let mut read = store.snapshot().await.unwrap();
        let children = read
            .mail_cancel_expansion_page(CancelExpansionSeq::ZERO, 100)
            .await
            .unwrap();
        let restored: Vec<_> = children
            .iter()
            .map(|row| {
                (
                    row.sequence,
                    row.expansion.group_handoff.clone(),
                    row.expansion.target_item.clone(),
                    serde_json::to_value(&row.expansion.envelope).unwrap(),
                )
            })
            .collect();
        assert_eq!(
            restored, frozen_children,
            "quota retries changed frozen child identities or associations"
        );
        read.finish().await.unwrap();
        assert!(runtime.requests.lock().await.try_recv().is_err());
        publisher.shutdown().await;
        drop(publisher);
        drop(app);
        drop(observer);
        drop(store);
        runtime.close().await;
        fixture.close_native(native).await;
        query.close().await;
    }))
    .await
    .expect("bounded actual outbox quota and frozen-child recovery");
}
