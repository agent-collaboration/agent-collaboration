//! Real library custody, journal admission and push rows share commit and rollback.
use crate::{InteractionAtomicStore, InteractionStore, push_records};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_protocol::{PushHeaderFacts, PushId, PushKind, PushOrigin, PushRecordDraft};
use durable_steps::{
    AtomicStore, CallFailure, CallId, CallTarget, Host, NotSentReason, StoreError,
    storage::TransactionScope,
};
use fleet_sync_delivery::{
    DeliveryConsumer, DeliveryError, DeliveryLimits, GateDecision, LocalOutcome, Outbox,
    OutboxAdmission, SubmitItem, minimum_settlement_allowance,
};
use fleet_sync_protocol::{
    DeliveryFailureKind, DeliveryHeader, DeliveryItem, DeliveryRefusal, PeerId, Recipient,
};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde_json::json;
use std::sync::Arc;

struct NoDispatchHost;
impl Clock for NoDispatchHost {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(0)
    }
}
impl IdSource for NoDispatchHost {
    fn new_id(&self) -> durable_steps::Uuid {
        uuid::Uuid::now_v7()
    }
}
impl WakeScheduler for NoDispatchHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for NoDispatchHost {
    async fn dispatch<TCall: CallTarget>(
        &self,
        _: TCall,
        _: TCall::Request,
        _: CallId,
    ) -> Result<TCall::Answer, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        })
    }
}

struct NoDeliveryConsumer;
impl<TScope: TransactionScope> DeliveryConsumer<TScope> for NoDeliveryConsumer {
    type Protocol = BoardConsumerProtocol;
    async fn release_gate(
        &self,
        _: &mut TScope,
        _: &DeliveryItem<BoardDeliveryTypes>,
    ) -> Result<GateDecision, DeliveryError> {
        Ok(GateDecision::Release)
    }
    async fn deliver_locally(
        &self,
        _: &DeliveryItem<BoardDeliveryTypes>,
        _: CallId,
    ) -> fleet_sync_delivery::LocalDelivery<BoardDeliveryTypes> {
        fleet_sync_delivery::LocalDelivery {
            observation: (),
            outcome: LocalOutcome::Refused {
                reason: DeliveryRefusal::ProcessingFailed {
                    kind: DeliveryFailureKind::ExecutionFailed,
                },
            },
        }
    }
    fn may_cancel(
        &self,
        requester: &<BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Sender,
        item: &DeliveryHeader<BoardDeliveryTypes>,
    ) -> bool {
        requester == item.sender()
    }
}

type NativeStore = InteractionAtomicStore<NoDispatchHost>;
#[path = "carrier_push_storage_tests.rs"]
mod carrier_recovery;
#[path = "held_mail_readiness_tests.rs"]
mod held_readiness;
#[path = "mail_delivery_observation_tests.rs"]
mod mail_observations;
#[path = "mail_push_retention_tests.rs"]
mod mail_retention;
#[path = "pending_poll_mail_tests.rs"]
mod pending_poll;
type NativeSubmit =
    SubmitItem<NativeStore, sqlx_turso::Turso, BoardDeliveryTypes, NoDeliveryConsumer>;

#[path = "composed_schema_upgrade_tests.rs"]
mod composed_schema_upgrade;
#[path = "local_cancel_delivery_storage_tests.rs"]
mod local_cancels;
#[path = "message_send_answer_custody_tests.rs"]
mod queued_answers;
#[path = "mail_submission_epoch_tests.rs"]
mod submission_epochs;

fn native_item_and_push() -> (DeliveryItem<BoardDeliveryTypes>, PushRecordDraft) {
    let id = uuid::Uuid::now_v7();
    let target = json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
        "sessionId":"atomic-recipient"
    });
    let item = DeliveryItem::<BoardDeliveryTypes>::new(
        fleet_sync_protocol::ItemId::new(id).expect("native item ID"),
        serde_json::from_value(
            json!({"kind":"humanUser","installation":"00000000-0000-4000-8000-000000000002"}),
        )
        .expect("sender"),
        Recipient {
            peer: PeerId::new(uuid::Uuid::now_v7()).expect("recipient peer"),
            address: serde_json::from_value(target.clone()).expect("board address"),
        },
        None,
        serde_json::from_value(
            json!({"kind":"message","text":"atomic body","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}),
        )
        .expect("board body"),
    )
    .expect("real library item");
    let push = PushRecordDraft {
        push_id: PushId::try_from(id.to_string()).expect("same push identity"),
        kind: PushKind::DirectMessage,
        origin: PushOrigin::OwnerUnverified,
        origin_router_ref: None,
        target: serde_json::from_value(target).expect("native address"),
        mode: Some(collaboration_protocol::MessageDelivery::Auto),
        guard: None,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some("atomic body".into()),
        activity: None,
        created_at: chrono::Utc::now(),
    };
    (item, push)
}

async fn enqueue_and_project(
    store: &NativeStore,
    item: DeliveryItem<BoardDeliveryTypes>,
    draft: PushRecordDraft,
) -> Result<(), StoreError> {
    store
        .transaction(move |scope| {
            Box::pin(async move {
                let item_id = item
                    .id()
                    .as_uuid()
                    .to_string()
                    .try_into()
                    .map_err(StoreError::backend)?;
                // Fixture quotas only; the reserve is derived from the real board types.
                let limits = DeliveryLimits::new::<BoardDeliveryTypes>(
                    16 * 1024 * 1024,
                    20,
                    16 * 1024 * 1024,
                    20,
                    16 * 1024 * 1024,
                    minimum_settlement_allowance::<BoardDeliveryTypes>()
                        .map_err(StoreError::backend)?,
                )
                .map_err(StoreError::backend)?;
                let (answer, admitted) = Outbox::<_, BoardDeliveryTypes>::new(scope, limits)
                    .enqueue_and_admit::<NativeSubmit>(
                        item,
                        PeerId::new(uuid::Uuid::now_v7()).expect("relay"),
                    )
                    .await
                    .map_err(StoreError::backend)?;
                assert!(matches!(answer, OutboxAdmission::Queued { .. }));
                assert!(admitted.is_some());
                let record = draft.into_pending().map_err(StoreError::backend)?;
                crate::record_message_send_admission(
                    scope,
                    &item_id,
                    "🤖 frozen recipient",
                    record.created_at,
                )
                .await
                .map_err(StoreError::backend)?;
                push_records::insert_push_record_with_connection(scope.executor(), &record)
                    .await
                    .map_err(|error| StoreError::backend(format!("push insertion: {error:?}")))?;
                Ok(())
            })
        })
        .await
}

#[tokio::test]
async fn library_outbox_and_journal_commit_with_push_and_roll_back_on_real_push_failure() {
    // Arrange the actual composed owner, library adapter and one successful transaction.
    let directory = tempfile::tempdir().expect("native atomic fixture");
    let path = directory.path().join("interaction.sqlite");
    let owner = Arc::new(InteractionStore::open(&path).await.expect("composed owner"));
    let adapter = InteractionAtomicStore::new(owner.clone(), Arc::new(NoDispatchHost));
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    let mut committed_writes = adapter.runtime_test_observe_committed_writes();
    let (committed_item, committed_push) = native_item_and_push();
    let committed_id = committed_push.push_id.clone();
    enqueue_and_project(&adapter, committed_item, committed_push)
        .await
        .expect("shared commit");
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    {
        assert!(
            committed_writes
                .has_changed()
                .expect("commit observation is live")
        );
        committed_writes.borrow_and_update();
        adapter
            .transaction(|scope| {
                Box::pin(async move {
                    let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM router_pushes")
                        .fetch_one(scope.executor())
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await
            .expect("read-only atomic observation");
        assert!(
            !committed_writes
                .has_changed()
                .expect("read observer remains live"),
            "status reads must not wake their own commit observation"
        );
    }
    let mut connection = owner.writer.lock().await;
    sqlx::raw_sql("CREATE TRIGGER reject_atomic_push BEFORE INSERT ON router_pushes BEGIN SELECT RAISE(ABORT, 'fixture atomic push rejection'); END")
        .execute(&mut *connection).await.expect("real native insertion fault");
    drop(connection);

    // Act: the real push write fails after custody and journal admission wrote.
    let (rejected_item, rejected_push) = native_item_and_push();
    let rejected_id = rejected_push.push_id.clone();
    let error = enqueue_and_project(&adapter, rejected_item, rejected_push)
        .await
        .expect_err("atomic rollback");
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    assert!(
        !committed_writes
            .has_changed()
            .expect("rollback observer remains live"),
        "failed transaction must not publish committed progress"
    );
    assert!(
        error.to_string().contains("fixture atomic push rejection"),
        "{error:?}"
    );
    let mut connection = owner.writer.lock().await;
    sqlx::query("DROP TRIGGER reject_atomic_push")
        .execute(&mut *connection)
        .await
        .expect("fault removal after completed rollback");
    drop(connection);
    drop(adapter);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("adapter retained owner"))
        .close()
        .await
        .expect("close native owner");

    // Assert against a cold owning reopen, not the transaction's cached objects.
    let reopened = InteractionStore::open(path)
        .await
        .expect("cold composed reopen");
    assert!(
        reopened
            .get_push_record(&committed_id)
            .await
            .expect("committed push read")
            .is_some()
    );
    assert!(
        reopened
            .get_push_record(&rejected_id)
            .await
            .expect("rejected push read")
            .is_none()
    );
    let mut connection = reopened.writer.lock().await;
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM router_pushes),(SELECT count(*) FROM _fleet_sync_outbox_items),(SELECT count(*) FROM _fleet_sync_operations)")
        .fetch_one(&mut *connection).await.expect("durable custody, journal and push counts");
    assert_eq!(counts, (1, 1, 1));
    let answers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message_send_answers")
        .fetch_one(&mut *connection)
        .await
        .expect("same-transaction RPC projection");
    assert_eq!(
        answers, 1,
        "failed custody/push admission must roll back its RPC projection"
    );
    drop(connection);
    reopened.close().await.expect("close proof owner");
}
