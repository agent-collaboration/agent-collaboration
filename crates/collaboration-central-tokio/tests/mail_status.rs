//! A disconnected delivery leaf completes only after preserving fetchable status.
#![cfg(feature = "conformance")]

use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_core::{CoreHost, ObjectAddress, ObjectRuntime, PlatformIo, SessionKind};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{
    AtomicStore, CallFailure, CallId, CallTarget, OperationId, StepName, StoreError, Uuid,
};
use fleet_sync_delivery::{PublicationTarget, PublishCall, StatusStream};
use fleet_sync_protocol::{
    DeliveryFailureKind, DeliveryRefusal, DeliverySeq, DeliveryStatus, ItemId, PeerId, ReceiverId,
    StatusCursor, StatusFetchResult, StatusPublication, StatusPublicationResult, StatusVersion,
};
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore, TokioObject};
use message_board::ServiceId;
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde_json::json;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Acceptance = <BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Acceptance;
type Runtime = ObjectRuntime<SessionKind, TokioStorageHost<CoreHost<NoObjectCalls>>, NoObjectCalls>;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "session_publication/support.rs"]
mod session_fixture;

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
                ObjectKey::new("connection-status-leaf")?,
                Default::default(),
                Arc::new(CoreHost::for_store(io.clone(), receiver)),
            )
            .await?;
        let runtime = ObjectRuntime::session(
            Arc::new(TokioStorageHost::new(object.clone())),
            io.clone(),
            receiver,
            installation.clone(),
            Duration::from_secs(2),
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

    async fn publish(
        &self,
        peer: PeerId,
        route: ServiceId,
        item: ItemId,
        call: &CallId,
    ) -> TestResult<StatusPublicationResult> {
        let target = PublishCall::<BoardDeliveryTypes>::new(PublicationTarget::StatusStream {
            sender: peer,
            route,
        });
        let publication = StatusPublication::<Acceptance> {
            item,
            status: DeliveryStatus::Sent {
                seq: DeliverySeq::new(1),
            },
            version: StatusVersion::new(1),
            cancel: None,
        };
        let bytes = serde_json::to_vec(&json!({
            "source":{"kind":"mailbox","value":"550e8400-e29b-41d4-a716-4466554400e5"},
            "sourceFace":"mailbox","authority":{"kind":"object"},
            "destination":{"kind":"connection","value":self.installation},
            "destinationFace":"delivery","identity":call,
            "kind":PublishCall::<BoardDeliveryTypes>::KIND,"target":target,"request":publication
        }))?;
        let answer = self
            .runtime
            .delivery()?
            .on_call(bytes)
            .await
            .map_err(|error| format!("mailbox publication was not answered: {error:?}"))?;
        Ok(serde_json::from_slice(&answer)?)
    }

    async fn fetch(&self, peer: PeerId) -> TestResult<StatusFetchResult<Acceptance>> {
        let route = self.installation.clone();
        Ok(self
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    StatusStream::<_, BoardDeliveryTypes>::new(scope, peer, route)
                        .fetch(peer, StatusCursor::new(0), 100)
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
    let identity = call()?;
    let first = Fixture::open(root.path(), installation.clone()).await?;
    assert_eq!(
        first
            .publish(peer, installation.clone(), item, &identity)
            .await?,
        StatusPublicationResult::Applied
    );
    let stored = first.fetch(peer).await?;
    assert_eq!(stored.events.len(), 1);
    assert_eq!(stored.through, StatusCursor::new(1));
    assert_eq!(stored.events[0].item, item);
    assert_eq!(stored.events[0].version, StatusVersion::new(1));
    assert_eq!(
        stored.events[0].status,
        DeliveryStatus::Sent {
            seq: DeliverySeq::new(1)
        }
    );

    // A disconnected peer publication finishes, rather than backing off forever.
    assert_eq!(first.runtime.on_alarm().await?.completed.len(), 1);
    assert!(first.runtime.on_alarm().await?.completed.is_empty());
    assert_eq!(
        first.io.0.load(Ordering::SeqCst),
        0,
        "leaf made an object call"
    );
    assert_eq!(
        first
            .publish(peer, installation.clone(), item, &identity)
            .await?,
        StatusPublicationResult::Applied
    );
    assert_eq!(first.fetch(peer).await?, stored);
    first.objects.close().await?;
    drop(first);

    let reopened = Fixture::open(root.path(), installation.clone()).await?;
    assert_eq!(reopened.fetch(peer).await?, stored);
    assert_eq!(
        reopened
            .publish(peer, installation, item, &identity)
            .await?,
        StatusPublicationResult::Applied
    );
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
    let fixture = Fixture::open(root.path(), installation).await?;
    assert_eq!(
        fixture
            .publish(peer, wrong_route, ItemId::new(Uuid::now_v7())?, &call()?)
            .await?,
        StatusPublicationResult::Refused {
            reason: DeliveryRefusal::ProcessingFailed {
                kind: DeliveryFailureKind::InvalidPayload
            }
        }
    );
    assert!(fixture.fetch(peer).await?.events.is_empty());
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
    assert_eq!(
        first
            .publish(peer, installation.clone(), item, &call()?)
            .await?,
        StatusPublicationResult::Applied
    );
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
    use fleet_sync_protocol::{
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
    let target = PublishCall::<BoardDeliveryTypes>::new(PublicationTarget::StatusStream {
        sender: binding.peer,
        route: route.clone(),
    });
    let before = fixture
        .io
        .calls
        .lock()
        .map_err(|_| "call mutex poisoned")?
        .len();
    let answer = fixture.runtime.delivery()?.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"mailbox","value":"550e8400-e29b-41d4-a716-4466554400e5"},
        "sourceFace":"mailbox","authority":{"kind":"object"},
        "destination":{"kind":"connection","value":route},"destinationFace":"delivery",
        "identity":call()?,"kind":PublishCall::<BoardDeliveryTypes>::KIND,"target":target,"request":publication
    }))?).await?;
    assert_eq!(
        serde_json::from_slice::<StatusPublicationResult>(&answer)?,
        StatusPublicationResult::Applied
    );
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
