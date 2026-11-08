use super::EventScopeStore;
use crate::event_owed::{scoped, take};
use crate::fleet_storage_fixture::{FleetStorageFixture, NOW, TestError};
use board_sync_protocol::BoardNotice;
use durable_steps::{AtomicStore, BoxFuture, StoreError};
use fleet_protocol::PeerId;
use std::{collections::BTreeSet, sync::Arc};

/// Runs each transaction on its own task, as workerd's storage transaction
/// runs its callback as a separate promise.
struct DetachedStore<Store>(Arc<Store>);

impl<Store: AtomicStore + Send + Sync + 'static> AtomicStore for DetachedStore<Store> {
    type Scope<'transaction> = Store::Scope<'transaction>;

    async fn transaction<Output, Callback>(&self, callback: Callback) -> Result<Output, StoreError>
    where
        Output: Send + 'static,
        Callback: for<'scope, 'transaction> FnOnce(
                &'scope mut Self::Scope<'transaction>,
            )
                -> BoxFuture<'scope, Result<Output, StoreError>>
            + Send
            + 'static,
    {
        let inner = Arc::clone(&self.0);
        tokio::spawn(async move { inner.transaction(callback).await })
            .await
            .map_err(StoreError::backend)?
    }
}

/// Append one notice for `peer` through `store`, inside one event.
async fn appended_in_event<Store: AtomicStore>(
    store: &Store,
    peer: PeerId,
) -> Result<BTreeSet<PeerId>, StoreError>
where
    for<'transaction> Store::Scope<'transaction>:
        durable_steps::storage::TransactionScope<Database = sqlx_turso::Turso>,
{
    scoped(async move {
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    crate::machine_notice_store::append(
                        scope,
                        peer,
                        durable_steps::Uuid::now_v7().to_string(),
                        BoardNotice::ProjectListChanged {},
                        NOW,
                    )
                    .await
                    .map(|_| ())
                })
            })
            .await?;
        Ok(take().notices)
    })
    .await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn an_append_in_a_detached_transaction_still_names_its_machine() -> Result<(), TestError> {
    // Arrange: a store that polls its transaction callback on another task.
    let fixture = FleetStorageFixture::open().await?;
    let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
    fixture
        .enroll(peer, "019f0000-0000-7000-8000-000000008141")
        .await?;
    let detached = Arc::new(DetachedStore(Arc::clone(&fixture.object)));

    // Act
    let lost = appended_in_event(detached.as_ref(), peer).await?;
    let kept = appended_in_event(&EventScopeStore::new(Arc::clone(&detached)), peer).await?;

    // Assert: the bare detached store loses the event's scope (the workerd
    // defect); carried across the transaction, the append names its machine.
    assert!(lost.is_empty(), "the detached task saw the event's scope");
    assert_eq!(kept, BTreeSet::from([peer]));
    fixture.close().await?;
    Ok(())
}

/// S1: an owed send bound inside a detached transaction still binds the send
/// record of the frame that carries it.
#[cfg(feature = "conformance")]
mod send_bindings {
    use super::{DetachedStore, EventScopeStore};
    use crate::ObjectAddress;
    use crate::event_trace::{
        BarrierPoint, EventBounds, EventKindRecord, EventRecord, EventTrace, KernelObserver,
        ReplyRecord, SendBinding, bind_send, record_send, scoped,
    };
    use crate::fleet_storage_fixture::{FleetStorageFixture, TestError};
    use board_sync_protocol::BoardConsumerProtocol;
    use durable_steps::{AtomicStore, BoxFuture, StoreError, Uuid};
    use fleet_protocol::{
        ConnectionId, DeliveryStatus, FrameId, ItemId, ServerCall, ServerFrame, ServerRequest,
        StatusPublication, StatusVersion, SyncTime,
    };
    use std::sync::{Arc, Mutex, PoisonError};

    #[derive(Default)]
    struct CollectingObserver {
        records: Mutex<Vec<EventRecord>>,
    }

    impl KernelObserver for CollectingObserver {
        fn object(&self) -> ObjectAddress {
            ObjectAddress::Fleet
        }
        fn construction(&self) -> Uuid {
            Uuid::nil()
        }
        fn emit(&self, record: EventRecord) {
            self.records
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(record);
        }
        fn barrier(
            &self,
            _point: BarrierPoint,
            _unit_kind: Option<&'static str>,
            _event: Uuid,
        ) -> BoxFuture<'static, ()> {
            Box::pin(std::future::ready(()))
        }
    }

    /// A real status-publication request frame under `send_id`.
    fn publish_frame(send_id: Uuid) -> Result<Vec<u8>, TestError> {
        let call = serde_json::from_value(
            serde_json::json!({"operation": Uuid::now_v7(), "step": "status-v1"}),
        )?;
        Ok(serde_json::to_vec(
            &ServerFrame::<BoardConsumerProtocol>::Request {
                frame: FrameId::new(send_id)?,
                body: ServerRequest::Crate(ServerCall::PublishStatus {
                    call,
                    publication: StatusPublication {
                        item: ItemId::new(Uuid::now_v7())?,
                        status: DeliveryStatus::Sent,
                        version: StatusVersion::new(1),
                        cancel: None,
                    },
                }),
            },
        )?)
    }

    /// One event: a transaction through `store` binds `send_id`, then the frame
    /// leaves. Answers the event's one send record.
    async fn send_in_event<Store: AtomicStore>(
        store: &Store,
        binding: SendBinding,
        send_id: Uuid,
        connection: ConnectionId,
        frame: Vec<u8>,
    ) -> Result<crate::event_trace::SendRecord, TestError> {
        let observer = Arc::new(CollectingObserver::default());
        let trace = Arc::new(EventTrace::new(
            Arc::clone(&observer) as Arc<dyn KernelObserver>,
            crate::event_trace::EventClass::Alarm,
        ));
        scoped(Arc::clone(&trace), async move {
            store
                .transaction(move |_scope| {
                    Box::pin(async move {
                        bind_send(send_id, binding);
                        Ok::<_, StoreError>(())
                    })
                })
                .await?;
            record_send(connection, &frame);
            Ok::<_, StoreError>(())
        })
        .await?;
        trace.finish(EventBounds {
            sequence: 0,
            event: EventKindRecord::Alarm { redelivery: None },
            entered_at: SyncTime::from_unix_millis(1),
            exited_at: SyncTime::from_unix_millis(2),
            alarm_before: None,
            alarm_after: None,
            reply: ReplyRecord::AlarmDone,
        });
        let mut records = observer
            .records
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let record = records.pop().ok_or("the event emitted no record")?;
        let [send] = <[_; 1]>::try_from(record.sends)
            .map_err(|sends| format!("expected one send, got {}", sends.len()))?;
        Ok(send)
    }

    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions must fail this test while fixture errors propagate through Result."
    )]
    #[tokio::test]
    async fn a_send_bound_in_a_detached_transaction_still_binds_its_record() -> Result<(), TestError>
    {
        // Arrange
        let fixture = FleetStorageFixture::open().await?;
        let detached = Arc::new(DetachedStore(Arc::clone(&fixture.object)));
        let connection = ConnectionId::new(Uuid::now_v7())?;
        let binding = SendBinding {
            attempt: Uuid::now_v7(),
            attempt_kind: "status",
            subject: Uuid::now_v7(),
            session: Uuid::now_v7(),
            request_kind: "publishStatus".to_owned(),
            sent_version: Some(1),
        };

        // Act
        let lost_id = Uuid::now_v7();
        let lost = send_in_event(
            detached.as_ref(),
            binding.clone(),
            lost_id,
            connection,
            publish_frame(lost_id)?,
        )
        .await?;
        let kept_id = Uuid::now_v7();
        let kept = send_in_event(
            &EventScopeStore::new(Arc::clone(&detached)),
            binding.clone(),
            kept_id,
            connection,
            publish_frame(kept_id)?,
        )
        .await?;

        // Assert: bound on the detached task alone, the binding is lost and the
        // record keeps the outer kind; carried across, it binds the inner kind.
        assert_eq!(lost.send_id, None);
        assert_eq!(lost.request_kind.as_deref(), Some("request"));
        assert_eq!(kept.send_id, Some(kept_id));
        assert_eq!(kept.attempt, Some(binding.attempt));
        assert_eq!(kept.attempt_kind.as_deref(), Some("status"));
        assert_eq!(kept.subject, Some(binding.subject));
        assert_eq!(kept.session, Some(binding.session));
        assert_eq!(kept.request_kind.as_deref(), Some("publishStatus"));
        assert_eq!(kept.sent_version, Some(1));
        fixture.close().await?;
        Ok(())
    }
}
