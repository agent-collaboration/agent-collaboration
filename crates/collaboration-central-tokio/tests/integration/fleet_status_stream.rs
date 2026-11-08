//! Fleet status debt stays fetchable across offline publication and owning-store reopen.
#![cfg(feature = "conformance")]

use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_core::{CoreHost, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{
    AtomicStore, CallFailure, CallId, OperationId, StepName, StoreError, Uuid,
    storage::TransactionScope,
};
use fleet_mail_delivery::{PublishStatus, StatusStream};
use fleet_protocol::{
    DeliveryStatus, ItemId, PeerId, ReceiverId, StatusCursor, StatusFetchResult, StatusPublication,
    StatusVersion,
};
use fleet_tokio_objects::{ObjectKey, ObjectStore, TokioObject};
use message_board::ServiceId;
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use sqlx_core::query::query;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Acceptance = <BoardDeliveryTypes as fleet_protocol::DeliveryTypes>::Acceptance;
type Runtime = ObjectRuntime<
    collaboration_central_core::FleetKind,
    TokioStorageHost<CoreHost<NoObjectCalls>>,
    NoObjectCalls,
>;

use crate::session_publication_fixture as session_fixture;

#[derive(Clone, Default)]
struct NoObjectCalls(Arc<AtomicUsize>);
impl Clock for NoObjectCalls {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_792_281_600_000)
    }
}
impl IdSource for NoObjectCalls {
    fn new_id(&self) -> Uuid {
        Uuid::now_v7()
    }
}
impl WakeScheduler for NoObjectCalls {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl PlatformIo for NoObjectCalls {
    fn reply_deadline(
        &self,
        remaining: std::time::Duration,
    ) -> impl std::future::Future<Output = ()> + Send {
        tokio::time::sleep(remaining)
    }

    async fn object_call(&self, _: ObjectAddress, _: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(CallFailure::AnswerLost)
    }
}

struct Fixture {
    objects: ObjectStore<CoreHost<NoObjectCalls>>,
    object: Arc<TokioObject<CoreHost<NoObjectCalls>>>,
    runtime: Runtime,
    io: NoObjectCalls,
    installation: ServiceId,
}
impl Fixture {
    async fn open(root: &Path, installation: ServiceId) -> TestResult<Self> {
        let io = NoObjectCalls::default();
        let receiver = ReceiverId::new(Uuid::now_v7())?;
        let objects = ObjectStore::open(root).await?;
        let object = objects
            .object(
                ObjectKey::new("fleet")?,
                Default::default(),
                Arc::new(CoreHost::for_store(io.clone(), receiver)),
            )
            .await?;
        let runtime = ObjectRuntime::fleet(
            Arc::new(TokioStorageHost::new(object.clone())),
            io.clone(),
            fleet_project_lease::RegistryConfig {
                authoritative_receiver: receiver,
                lease: fleet_project_lease::LeasePolicy {
                    lease_duration: std::time::Duration::from_secs(60),
                    renew_interval: std::time::Duration::from_secs(20),
                },
                release_unsent_for: std::time::Duration::from_secs(10),
                move_needs_renewal_within: None,
            },
            collaboration_central_tokio::DeliveryConfig {
                relay_peer: "019f0000-0000-7000-8000-000000005240"
                    .to_owned()
                    .try_into()?,
            },
            Duration::from_secs(2),
            fleet_protocol::FrameCapabilities::default(),
        )?;
        runtime.initialize().await?;
        Ok(Self {
            objects,
            object,
            runtime,
            io,
            installation,
        })
    }

    async fn publish(&self, peer: PeerId, route: ServiceId, item: ItemId) -> TestResult {
        let installation = self.installation.clone();
        let now = self.io.now();
        self.object.transaction(move |scope| Box::pin(async move {
            // Enrollment is the fixture input; stream storage, journal admission and dispatch are real.
            query::<sqlx_turso::Turso>("INSERT INTO machine_enrollments(machine_id,service_id,machine_label,access_client_id,credential_hash,state,expires_at,created_at,updated_at) VALUES(?,?,'status fixture',?,?,'enrolled','2033-05-18T03:33:20+00:00','2026-10-18T00:00:00+00:00','2026-10-18T00:00:00+00:00') ON CONFLICT(machine_id) DO NOTHING")
                .bind(peer.to_string()).bind(installation.as_str()).bind(format!("status-client-{peer}")).bind(format!("status-credential-{peer}"))
                .execute(scope.executor()).await.map_err(StoreError::backend)?;
            StatusStream::<_, BoardDeliveryTypes>::new(scope, fleet_protocol::Submitter::Machine(peer), route)
                .append_and_admit::<PublishStatus<TokioObject<CoreHost<NoObjectCalls>>, sqlx_turso::Turso, BoardDeliveryTypes>>(
                    &StatusPublication { item, status: DeliveryStatus::Sent, version: StatusVersion::new(1), cancel: None },
                    now,
                ).await.map_err(fleet_mail_delivery::delivery_rollback)?;
            Ok(())
        })).await?;
        Ok(())
    }

    async fn fetch(&self, peer: PeerId) -> TestResult<StatusFetchResult<Acceptance>> {
        let route = self.installation.clone();
        Ok(self
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    StatusStream::<_, BoardDeliveryTypes>::new(
                        scope,
                        fleet_protocol::Submitter::Machine(peer),
                        route,
                    )
                    .fetch(
                        fleet_protocol::Submitter::Machine(peer),
                        StatusCursor::new(0),
                        100,
                    )
                    .await
                    .map_err(StoreError::backend)
                })
            })
            .await?)
    }
}

fn call() -> TestResult<CallId> {
    Ok(CallId {
        operation: OperationId::new(Uuid::now_v7())?,
        step: StepName::new("publish")?,
    })
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_disconnected_leaf_completes_publication_and_fetches_one_revision_after_reopen()
-> TestResult {
    // The real status rules, store, journal and leaf run here. No socket is
    // attached; a live WebSocket and normal session fetch remain higher proof.
    let root = tempfile::tempdir()?;
    let installation = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e4".to_owned())?;
    let peer = PeerId::new(Uuid::now_v7())?;
    let item = ItemId::new(Uuid::now_v7())?;
    let first = Fixture::open(root.path(), installation.clone()).await?;
    first.publish(peer, installation.clone(), item).await?;
    let stored = first.fetch(peer).await?;
    assert_eq!(stored.events.len(), 1);
    assert_eq!(stored.through, StatusCursor::new(1));
    assert_eq!(stored.events[0].item, item);
    assert_eq!(stored.events[0].version, StatusVersion::new(1));
    assert_eq!(stored.events[0].status, DeliveryStatus::Sent);

    // A disconnected peer publication finishes, rather than backing off forever.
    assert_eq!(first.runtime.on_alarm().await?.completed.len(), 1);
    assert!(first.runtime.on_alarm().await?.completed.is_empty());
    assert_eq!(
        first.io.0.load(Ordering::SeqCst),
        0,
        "leaf made an object call"
    );
    first.publish(peer, installation.clone(), item).await?;
    assert_eq!(first.fetch(peer).await?, stored);
    first.objects.close().await?;
    drop(first);

    let reopened = Fixture::open(root.path(), installation.clone()).await?;
    assert_eq!(reopened.fetch(peer).await?, stored);
    reopened.publish(peer, installation, item).await?;
    assert!(reopened.runtime.on_alarm().await?.completed.is_empty());
    assert_eq!(reopened.fetch(peer).await?, stored);
    assert_eq!(reopened.io.0.load(Ordering::SeqCst), 0);
    reopened.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_status_route_for_another_installation_is_refused_without_forwarding() -> TestResult {
    let root = tempfile::tempdir()?;
    let installation = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e3".to_owned())?;
    let wrong_route = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e2".to_owned())?;
    let peer = PeerId::new(Uuid::now_v7())?;
    let fixture = Fixture::open(root.path(), installation.clone()).await?;
    fixture
        .publish(peer, installation, ItemId::new(Uuid::now_v7())?)
        .await?;
    let before = fixture.fetch(peer).await?;
    let refusal = fixture
        .publish(peer, wrong_route, ItemId::new(Uuid::now_v7())?)
        .await;
    let Err(error) = refusal else {
        return Err("another route overwrote the retained stream owner".into());
    };
    let store_error = error
        .downcast_ref::<StoreError>()
        .ok_or("wrong-route failure was not the owning store's rollback")?;
    let classified = fleet_mail_delivery::classify_delivery_store_error(store_error.clone());
    assert!(
        matches!(&classified, fleet_mail_delivery::DeliveryTransactionFailure::Corrupt { field }
        if field == "status stream owner or cursor"),
        "{classified:?}"
    );
    assert_eq!(fixture.fetch(peer).await?, before);
    assert_eq!(fixture.runtime.on_alarm().await?.completed.len(), 1);
    assert!(fixture.runtime.on_alarm().await?.completed.is_empty());
    assert_eq!(fixture.io.0.load(Ordering::SeqCst), 0);
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn reopening_between_storage_and_the_live_attempt_keeps_status_fetchable() -> TestResult {
    let root = tempfile::tempdir()?;
    let installation = ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400e1".to_owned())?;
    let peer = PeerId::new(Uuid::now_v7())?;
    let item = ItemId::new(Uuid::now_v7())?;
    let first = Fixture::open(root.path(), installation.clone()).await?;
    first.publish(peer, installation.clone(), item).await?;
    let saved = first.fetch(peer).await?;
    // No alarm has sent or retired this publication when the real store closes.
    first.objects.close().await?;
    drop(first);

    let reopened = Fixture::open(root.path(), installation).await?;
    assert_eq!(reopened.fetch(peer).await?, saved);
    assert_eq!(reopened.runtime.on_alarm().await?.completed.len(), 1);
    assert!(reopened.runtime.on_alarm().await?.completed.is_empty());
    assert_eq!(reopened.fetch(peer).await?, saved);
    assert_eq!(reopened.io.0.load(Ordering::SeqCst), 0);
    reopened.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_live_status_attempt_never_waits_for_the_router_and_session_fetch_keeps_its_cursor()
-> TestResult {
    use board_sync_protocol::BoardConsumerProtocol;
    use collaboration_central_tokio::SocketCommand;
    use fleet_protocol::{
        ClientFrame, DeliveryAnswer, DeliveryRequest, FrameId, OperationAnswer, OperationRequest,
        ServerCall, ServerFrame, ServerRequest,
    };
    // Registry report/Welcome replies are the existing fixture stand-in; the
    // session rules, native socket channel, status store and journal stay real.
    let root = tempfile::tempdir()?;
    let fixture = session_fixture::Fixture::open(root.path()).await?;
    let (binding, session, mut socket) = fixture.socket().await?;
    fixture.hello(binding, session).await?;
    fixture.runtime.on_alarm().await?;
    assert!(matches!(socket.try_recv(), Ok(SocketCommand::Frame(_))));
    let route = ServiceId::try_from("019f0000-0000-7000-8000-000000000001".to_owned())?;
    let item = ItemId::new(Uuid::now_v7())?;
    let publication = StatusPublication::<Acceptance> {
        item,
        status: DeliveryStatus::Held,
        version: StatusVersion::new(1),
        cancel: None,
    };
    let before = fixture
        .io
        .calls
        .lock()
        .map_err(|_| "call mutex poisoned")?
        .len();
    let publication_for_store = publication.clone();
    let now = fixture.io.now();
    fixture.host.store().transaction(move |scope| Box::pin(async move {
        StatusStream::<_, BoardDeliveryTypes>::new(scope, fleet_protocol::Submitter::Machine(binding.peer), route)
            .append_and_admit::<PublishStatus<session_fixture::TestStore, sqlx_turso::Turso, BoardDeliveryTypes>>(&publication_for_store, now)
            .await.map_err(fleet_mail_delivery::delivery_rollback)?;
        Ok(())
    })).await?;
    // The configured answer deadline is two seconds. This drive must finish
    // without any Router answer, well before that independent deadline.
    let driven = tokio::time::timeout(Duration::from_secs(1), fixture.runtime.on_alarm()).await??;
    assert_eq!(driven.completed.len(), 1);
    assert!(driven.retried.is_empty());
    assert_eq!(fixture.runtime.conformance_holder_observation()?.0, 0);
    let Some(SocketCommand::Frame(bytes)) = socket.try_recv().ok() else {
        return Err("live status request missing".into());
    };
    let ServerFrame::<BoardConsumerProtocol>::Request {
        body:
            ServerRequest::Crate(ServerCall::PublishStatus {
                publication: sent, ..
            }),
        ..
    } = serde_json::from_slice(&bytes)?
    else {
        return Err("live request was not PublishStatus".into());
    };
    assert_eq!(sent, publication);
    assert_eq!(
        fixture
            .io
            .calls
            .lock()
            .map_err(|_| "call mutex poisoned")?
            .len(),
        before,
        "status leaf dispatched another object"
    );

    let frame = FrameId::new(Uuid::now_v7())?;
    fixture
        .runtime
        .on_frame(
            binding,
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Delivery(DeliveryRequest::StatusFetch {
                    call: call()?,
                    after: StatusCursor::new(0),
                    limit: 10,
                }),
            })?,
        )
        .await?;
    let Some(SocketCommand::Frame(bytes)) = socket.try_recv().ok() else {
        return Err("status fetch answer missing".into());
    };
    let ServerFrame::<BoardConsumerProtocol>::Answer {
        frame: actual,
        body: OperationAnswer::Delivery(DeliveryAnswer::StatusFetch(fetched)),
    } = serde_json::from_slice(&bytes)?
    else {
        return Err("wrong correlated fetch answer".into());
    };
    assert_eq!(actual, frame);
    assert_eq!(fetched.through, StatusCursor::new(1));
    assert_eq!(fetched.events.len(), 1);
    let event = fetched.events.first().ok_or("stored event missing")?;
    assert_eq!(event.item, item);
    assert_eq!(event.version, StatusVersion::new(1));
    assert_eq!(event.status, DeliveryStatus::Held);
    fixture.objects.close().await?;
    Ok(())
}
