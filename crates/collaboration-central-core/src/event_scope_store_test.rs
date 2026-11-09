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

impl<Store: durable_steps::ReborrowScope + Send + Sync + 'static> durable_steps::ReborrowScope
    for DetachedStore<Store>
{
    fn reborrow<'scope, 'transaction: 'scope>(
        scope: &'scope mut Self::Scope<'transaction>,
    ) -> Self::Scope<'scope> {
        Store::reborrow(scope)
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
    let lost = appended_in_event(detached.as_ref(), peer).await;
    let kept = appended_in_event(&EventScopeStore::new(Arc::clone(&detached)), peer).await?;

    // Assert: the bare detached store loses the event's scope (the workerd
    // defect), which now refuses the append instead of dropping its notice;
    // carried across the transaction, the append names its machine.
    assert!(
        matches!(&lost, Err(error) if error.to_string().contains(&crate::event_owed::NoEventScope.to_string())),
        "the detached task saw the event's scope: {lost:?}"
    );
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
            event: EventKindRecord::Alarm {
                redelivery: None,
                delivery: crate::event_trace::AlarmDelivery::Platform,
            },
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
    async fn a_send_bound_in_a_detached_local_section_still_binds_its_record()
    -> Result<(), TestError> {
        // Arrange: stage_request's shape, a binding made inside a session
        // transaction whose local section runs as its own task.
        let fixture = FleetStorageFixture::open().await?;
        let store = super::local_sections::session_store(&fixture);
        let connection = ConnectionId::new(Uuid::now_v7())?;
        let binding = SendBinding {
            attempt: Uuid::now_v7(),
            attempt_kind: "push",
            subject: Uuid::now_v7(),
            session: Uuid::now_v7(),
            request_kind: "push".to_owned(),
            sent_version: None,
        };
        let send_id = Uuid::now_v7();

        // Act
        let send = send_in_event(
            &store,
            binding.clone(),
            send_id,
            connection,
            publish_frame(send_id)?,
        )
        .await?;

        // Assert
        assert_eq!(send.send_id, Some(send_id));
        assert_eq!(send.attempt, Some(binding.attempt));
        assert_eq!(send.request_kind.as_deref(), Some("push"));
        drop(store);
        fixture.close().await?;
        Ok(())
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

/// workerd polls both a local section (`blockConcurrencyWhile`) and a storage
/// transaction (`Storage::transaction`) as their own promises: a session
/// transaction's work runs two detachments away from the event's poll.
mod local_sections {
    use super::{DetachedStore, EventScopeStore};
    use crate::event_owed::{NoEventScope, scoped, take};
    use crate::fleet_storage_fixture::{FixtureIo, FleetStorageFixture, NOW, TestError};
    use crate::session_store::CoreSessionStore;
    use crate::work_attempts::{AttemptKind, socket_requests};
    use crate::{LocalSection, LocalSectionFailure, ObjectPlatform, SocketFailure};
    use durable_steps::{AtomicStore, StoreError, Uuid, storage::TransactionScope};
    use fleet_protocol::{ConnectionId, PeerId};
    use fleet_session::Bytes;
    use sqlx_core::{query::query, row::Row};
    use std::{collections::BTreeSet, sync::Arc};

    type Object = fleet_tokio_objects::TokioObject<crate::CoreHost<FixtureIo>>;
    type Store = EventScopeStore<DetachedStore<Object>>;

    /// A platform whose local section runs on another task, over a store whose
    /// transactions run on another task; it has no sockets.
    pub(super) struct DetachedSectionHost {
        store: Arc<Store>,
        io: Arc<FixtureIo>,
    }

    impl ObjectPlatform for DetachedSectionHost {
        type Database = sqlx_turso::Turso;
        type Store = Store;
        type Alarm = FixtureIo;
        type Io = FixtureIo;
        fn store(&self) -> &Arc<Store> {
            &self.store
        }
        fn io(&self) -> &Arc<FixtureIo> {
            &self.io
        }
        fn alarm(&self) -> &FixtureIo {
            self.io.as_ref()
        }
        async fn with_local_section<T: Send + 'static>(
            &self,
            run: LocalSection<T>,
        ) -> Result<T, LocalSectionFailure> {
            tokio::spawn(run)
                .await
                .map_err(|_| LocalSectionFailure::Interrupted)
        }
        async fn quarantine_connections(&self, _: Vec<ConnectionId>) {}
        async fn send_frame(&self, _: ConnectionId, _: Bytes) -> Result<(), SocketFailure> {
            Err(SocketFailure::NotConnected)
        }
        async fn close_connection(
            &self,
            _: ConnectionId,
            _: fleet_protocol::CloseReason,
        ) -> Result<(), SocketFailure> {
            Err(SocketFailure::NotConnected)
        }
        async fn registrations(&self) -> Result<crate::RegistrationSnapshot, SocketFailure> {
            Ok(Default::default())
        }
        async fn admission_origin(
            &self,
            _: ConnectionId,
        ) -> Result<crate::AdmissionOrigin, SocketFailure> {
            Err(SocketFailure::NotConnected)
        }
        async fn admission_binding(
            &self,
            _: ConnectionId,
        ) -> Result<fleet_session::ConnectionBinding, SocketFailure> {
            Err(SocketFailure::NotConnected)
        }
        async fn publish_registrations(
            &self,
            _: crate::RegistrationEdits,
        ) -> Vec<(ConnectionId, SocketFailure)> {
            Vec::new()
        }
    }

    /// The session store over the fixture's Fleet store, detached twice.
    pub(super) fn session_store(
        fixture: &FleetStorageFixture,
    ) -> CoreSessionStore<DetachedSectionHost> {
        CoreSessionStore::new(
            Arc::new(DetachedSectionHost {
                store: Arc::new(EventScopeStore::new(Arc::new(DetachedStore(Arc::clone(
                    &fixture.object,
                ))))),
                io: Arc::new(FixtureIo),
            }),
            fixture.object.host(),
            Arc::new(crate::holder_reply_hub::HolderReplyHub::default()),
        )
    }

    /// The answer path's shape: one session transaction owes `peer` a status
    /// publication (a waiting row) and names it to the event; `fail` rolls it back.
    async fn owe_status(
        store: &CoreSessionStore<DetachedSectionHost>,
        peer: PeerId,
        subject: Uuid,
        fail: bool,
    ) -> Result<(), StoreError> {
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    socket_requests::owe(scope, subject, AttemptKind::Status, NOW).await?;
                    crate::event_owed::owed_request(peer).map_err(StoreError::backend)?;
                    if fail {
                        return Err(StoreError::backend("rolled back after owing"));
                    }
                    Ok(())
                })
            })
            .await
    }

    async fn status_rows(fixture: &FleetStorageFixture, subject: Uuid) -> Result<i64, TestError> {
        Ok(fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>(
                        "SELECT count(*) AS rows FROM central_work_attempts WHERE subject=?1 AND kind='status'",
                    )
                    .bind(subject.to_string())
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .try_get("rows")
                    .map_err(StoreError::backend)
                })
            })
            .await?)
    }

    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions must fail this test while fixture errors propagate through Result."
    )]
    #[tokio::test]
    async fn owed_work_two_detachments_away_reaches_its_own_event_and_only_its_own()
    -> Result<(), TestError> {
        // Arrange
        let fixture = FleetStorageFixture::open().await?;
        let store = session_store(&fixture);
        let (a, b, nested) = (
            PeerId::new(Uuid::now_v7())?,
            PeerId::new(Uuid::now_v7())?,
            PeerId::new(Uuid::now_v7())?,
        );
        let (a_row, b_row, nested_row, rolled_back) = (
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
        );

        // Act: two interleaved events. A also runs a nested target-object event
        // (its own collector) between two owes, and one rolled-back owe.
        let event_a = scoped(async {
            owe_status(&store, a, a_row, false).await?;
            tokio::task::yield_now().await;
            let nested_seen = scoped(async {
                owe_status(&store, nested, nested_row, false).await?;
                Ok::<_, StoreError>(take().requests)
            })
            .await?;
            let refused = owe_status(&store, a, rolled_back, true).await;
            Ok::<_, StoreError>((take().requests, nested_seen, refused.is_err()))
        });
        let event_b = scoped(async {
            tokio::task::yield_now().await;
            owe_status(&store, b, b_row, false).await?;
            Ok::<_, StoreError>(take().requests)
        });
        let (a_result, b_result) = tokio::join!(event_a, event_b);
        let (a_seen, nested_seen, a_rolled_back) = a_result?;
        let b_seen = b_result?;

        // Assert: each event's post-event send sees exactly the machines it
        // owes; the nested event kept its own and A's scope came back after it.
        assert_eq!(a_seen, BTreeSet::from([a]));
        assert_eq!(b_seen, BTreeSet::from([b]));
        assert_eq!(nested_seen, BTreeSet::from([nested]));
        // The owed rows committed; the rolled-back owe left none.
        assert_eq!(status_rows(&fixture, a_row).await?, 1);
        assert_eq!(status_rows(&fixture, b_row).await?, 1);
        assert_eq!(status_rows(&fixture, nested_row).await?, 1);
        assert!(a_rolled_back);
        assert_eq!(status_rows(&fixture, rolled_back).await?, 0);
        drop(store);
        fixture.close().await?;
        Ok(())
    }

    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions must fail this test while fixture errors propagate through Result."
    )]
    #[tokio::test]
    async fn owing_outside_an_event_rolls_the_transaction_back_instead_of_losing_the_send()
    -> Result<(), TestError> {
        // Arrange: no event is running.
        let fixture = FleetStorageFixture::open().await?;
        let store = session_store(&fixture);
        let subject = Uuid::now_v7();

        // Act
        let refused = owe_status(&store, PeerId::new(Uuid::now_v7())?, subject, false).await;

        // Assert: loud, and nothing committed that no send would ever follow.
        assert!(
            matches!(&refused, Err(error) if error.to_string().contains(&NoEventScope.to_string())),
            "{refused:?}"
        );
        assert_eq!(status_rows(&fixture, subject).await?, 0);
        drop(store);
        fixture.close().await?;
        Ok(())
    }
}
