//! New outgoing calls need current Fleet enrollment; existing correlations remain independent.
use crate::fleet_storage_fixture::{FixtureIo, FleetStorageFixture, NOW, TestError};
use crate::{
    AdmissionOrigin, LocalSectionFailure, ObjectPlatform, RegistrationEdits, RegistrationSnapshot,
    SocketFailure,
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_protocol::{ConnectionId, PeerId, ReleaseRequest, ServerCall, SessionId};
use fleet_session::{Bytes, ConnectionBinding, ConnectionRegistration, VerifiedPeer};

use sqlx_core::executor::Executor;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
type Store = fleet_tokio_objects::TokioObject<crate::CoreHost<FixtureIo>>;
const PEER: &str = "019f0000-0000-7000-8000-00000000a201";
const SERVICE: &str = "019f0000-0000-7000-8000-00000000a202";
struct SelectionHost {
    store: Arc<Store>,
    connection: ConnectionId,
    session: SessionId,
    peer: PeerId,
    sends: AtomicUsize,
    io: Arc<FixtureIo>,
}
impl ObjectPlatform for SelectionHost {
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
        run: crate::LocalSection<T>,
    ) -> Result<T, LocalSectionFailure> {
        Ok(run.await)
    }
    async fn quarantine_connections(&self, _: Vec<ConnectionId>) {}
    async fn send_frame(&self, _: ConnectionId, _: Bytes) -> Result<(), SocketFailure> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn close_connection(
        &self,
        _: ConnectionId,
        _: fleet_protocol::CloseReason,
    ) -> Result<(), SocketFailure> {
        Ok(())
    }
    async fn registrations(&self) -> Result<RegistrationSnapshot, SocketFailure> {
        Ok([(
            self.connection,
            ConnectionRegistration::Attached {
                capabilities: None,
                session_id: self.session,
                schema_support: fleet_protocol::SchemaSupport { max_version: 7 },
            },
        )]
        .into_iter()
        .collect())
    }
    async fn admission_origin(&self, _: ConnectionId) -> Result<AdmissionOrigin, SocketFailure> {
        Ok(AdmissionOrigin {
            installation: SERVICE
                .to_owned()
                .try_into()
                .map_err(|_| SocketFailure::NotConnected)?,
            machine: "selection fixture"
                .to_owned()
                .try_into()
                .map_err(|_| SocketFailure::NotConnected)?,
        })
    }
    async fn admission_binding(&self, _: ConnectionId) -> Result<ConnectionBinding, SocketFailure> {
        Ok(ConnectionBinding::new(
            self.connection,
            VerifiedPeer {
                peer: self.peer,
                expires_at: NOW.after(std::time::Duration::from_secs(3600)),
            },
        ))
    }
    async fn publish_registrations(
        &self,
        _: RegistrationEdits,
    ) -> Vec<(ConnectionId, SocketFailure)> {
        Vec::new()
    }
}
fn call() -> Result<ServerCall<board_sync_protocol::BoardDeliveryTypes>, TestError> {
    Ok(ServerCall::Release {
        call: durable_steps::CallId {
            operation: durable_steps::OperationId::new(durable_steps::Uuid::now_v7())?,
            step: durable_steps::StepName::new("release")?,
        },
        request: serde_json::from_value::<ReleaseRequest>(
            serde_json::json!({"transfer":"019f0000-0000-7000-8000-00000000a203","database":"019f0000-0000-7000-8000-00000000a204","epoch":1}),
        )?,
    })
}
async fn run(revoked: bool) -> Result<(), TestError> {
    let fixture = FleetStorageFixture::open().await?;
    fixture.enroll(PEER.to_owned().try_into()?, SERVICE).await?;
    if revoked {
        fixture
            .object
            .transaction(|s| {
                Box::pin(async move {
                    s.executor()
                        .execute("UPDATE machine_enrollments SET state='revoked'")
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
    }
    let host = Arc::new(SelectionHost {
        store: fixture.object.clone(),
        connection: ConnectionId::new(durable_steps::Uuid::now_v7())?,
        session: SessionId::new(durable_steps::Uuid::now_v7())?,
        peer: PEER.to_owned().try_into()?,
        sends: AtomicUsize::new(0),
        io: Arc::new(FixtureIo),
    });
    let release = call()?;
    let ServerCall::Release {
        call: identity,
        request: release_request,
    } = &release
    else {
        return Err("release fixture mismatch".into());
    };
    let withdrawal = ServerCall::WithdrawRelease {
        call: identity.clone(),
        request: fleet_protocol::WithdrawRelease {
            database: release_request.database,
            epoch: release_request.epoch,
            transfer: release_request.transfer,
        },
    };
    let publication = ServerCall::PublishStatus {
        call: identity.clone(),
        publication: serde_json::from_value(
            serde_json::json!({"item":"019f0000-0000-7000-8000-00000000a203","status":{"kind":"queued"},"version":1,"cancel":null}),
        )?,
    };
    for (selected, request) in [release, withdrawal, publication].into_iter().enumerate() {
        let result = super::begin_peer_request(
            host.clone(),
            Arc::new(FixtureIo),
            Arc::new(crate::holder_reply_hub::HolderReplyHub::default()),
            host.peer,
            request,
            std::time::Duration::from_secs(2),
            |_| std::future::pending(),
        )
        .await;
        if revoked {
            if !matches!(result, Err(durable_steps::CallFailure::NotSent { .. })) {
                return Err("revoked peer acquired a new outgoing correlation/send".into());
            }
            if host.sends.load(Ordering::SeqCst) != 0 {
                return Err("revoked peer received a new call".into());
            }
        } else if result.is_err() || host.sends.load(Ordering::SeqCst) != selected + 1 {
            return Err("enrolled control did not select its call".into());
        }
    }
    fixture.close().await
}
#[tokio::test]
async fn revoked_peer_cannot_acquire_a_new_outgoing_call() -> Result<(), TestError> {
    run(true).await
}
#[tokio::test]
async fn enrolled_peer_can_acquire_an_outgoing_call() -> Result<(), TestError> {
    run(false).await
}

struct SelectionClock {
    now: std::sync::atomic::AtomicI64,
    samples: AtomicUsize,
}
impl runtime_primitives::Clock for SelectionClock {
    fn now(&self) -> runtime_primitives::SyncTime {
        self.samples.fetch_add(1, Ordering::SeqCst);
        runtime_primitives::SyncTime::from_unix_millis(self.now.load(Ordering::SeqCst))
    }
}
impl runtime_primitives::IdSource for SelectionClock {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
#[tokio::test]
async fn outgoing_selection_samples_expiry_after_waiting_for_the_fleet_transaction()
-> Result<(), TestError> {
    use std::future::Future;
    let fixture = FleetStorageFixture::open().await?;
    fixture.enroll(PEER.to_owned().try_into()?, SERVICE).await?;
    let expires = NOW.unix_millis() + 1;
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let expiry = chrono::DateTime::from_timestamp_millis(expires)
                    .ok_or_else(|| StoreError::backend("invalid fixture time"))?
                    .to_rfc3339();
                sqlx_core::query::query::<sqlx_turso::Turso>(
                    "UPDATE machine_enrollments SET expires_at=?",
                )
                .bind(expiry)
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    let host = Arc::new(SelectionHost {
        store: fixture.object.clone(),
        connection: ConnectionId::new(durable_steps::Uuid::now_v7())?,
        session: SessionId::new(durable_steps::Uuid::now_v7())?,
        peer: PEER.to_owned().try_into()?,
        sends: AtomicUsize::new(0),
        io: Arc::new(FixtureIo),
    });
    let clock = Arc::new(SelectionClock {
        now: std::sync::atomic::AtomicI64::new(NOW.unix_millis()),
        samples: AtomicUsize::new(0),
    });
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let held_store = fixture.object.clone();
    let held = tokio::spawn(async move {
        held_store
            .transaction(move |_| {
                Box::pin(async move {
                    entered_tx
                        .send(())
                        .map_err(|_| StoreError::backend("entry observer dropped"))?;
                    release_rx.await.map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await
    });
    entered_rx.await?;
    let mut selecting = Box::pin(super::begin_peer_request(
        host.clone(),
        clock.clone(),
        Arc::new(crate::holder_reply_hub::HolderReplyHub::default()),
        host.peer,
        call()?,
        std::time::Duration::from_secs(2),
        |_| std::future::pending(),
    ));
    let pending = std::future::poll_fn(|context| {
        std::task::Poll::Ready(selecting.as_mut().poll(context).is_pending())
    })
    .await;
    let samples_while_blocked = clock.samples.load(Ordering::SeqCst);
    clock.now.store(expires, Ordering::SeqCst);
    release_tx
        .send(())
        .map_err(|_| "held transaction ended early")?;
    held.await??;
    let result = selecting.await;
    if !pending || samples_while_blocked != 0 {
        return Err("outgoing selection sampled time before entering its SQL transaction".into());
    }
    if !matches!(result, Err(durable_steps::CallFailure::NotSent { .. }))
        || host.sends.load(Ordering::SeqCst) != 0
    {
        return Err(
            "enrollment expiring during transaction wait still selected an outgoing call".into(),
        );
    }
    fixture.close().await
}

#[path = "forward_selection_test.rs"]
mod forward_tests;
