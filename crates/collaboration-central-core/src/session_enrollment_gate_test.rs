//! Real Fleet transaction selection must refuse revoked callers before shared work is staged.
use crate::fleet_storage_fixture::{FixtureIo, FleetStorageFixture, NOW, TestError};
use crate::session_scope::CoreSessionScope;
use crate::{LocalSectionFailure, ObjectHost, SocketFailure};
use durable_steps::storage::TransactionScope;
use durable_steps::{AtomicStore, ReborrowScope, StoreError};
use fleet_sync_protocol::{ConnectionId, FrameId, OperationRequest, SessionId};
use fleet_sync_session::{Bytes, SessionEffect, SessionKey};
use serde_json::{Value, json};
use sqlx_core::executor::Executor;
use std::{marker::PhantomData, sync::Arc};
type Store = fleet_sync_tokio_runner::TokioObject<crate::CoreHost<FixtureIo>>;
const PEER: &str = "019f0000-0000-7000-8000-00000000a101";
const SERVICE: &str = "019f0000-0000-7000-8000-00000000a102";
const PROJECT: &str = "019f0000-0000-7000-8000-00000000a103";
// No socket or host method is used: the deciding transaction and its committed-effect selection are real.
struct ScopeHost {
    store: Arc<Store>,
    wake: FixtureIo,
}
impl ObjectHost for ScopeHost {
    type Database = sqlx_turso::Turso;
    type Store = Store;
    type Wake = FixtureIo;
    fn store(&self) -> &Arc<Store> {
        &self.store
    }
    fn wake(&self) -> &FixtureIo {
        &self.wake
    }
    async fn with_local_section<T: Send + 'static>(
        &self,
        run: impl std::future::Future<Output = T> + Send + 'static,
    ) -> Result<T, LocalSectionFailure> {
        Ok(run.await)
    }
    async fn quarantine_connections(&self, _: Vec<ConnectionId>) {}
    async fn send_frame(&self, _: ConnectionId, _: Bytes) -> Result<(), SocketFailure> {
        Err(SocketFailure::NotConnected)
    }
    async fn close_connection(
        &self,
        _: ConnectionId,
        _: fleet_sync_protocol::CloseReason,
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
    ) -> Result<fleet_sync_session::ConnectionBinding, SocketFailure> {
        Err(SocketFailure::NotConnected)
    }
    async fn publish_registrations(
        &self,
        _: crate::RegistrationEdits,
    ) -> Vec<(ConnectionId, SocketFailure)> {
        Vec::new()
    }
}
async fn selection(body: Value, expected: Value) -> Result<(), TestError> {
    let fixture = FleetStorageFixture::open().await?;
    fixture.enroll(PEER.to_owned().try_into()?, SERVICE).await?;
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                scope
                    .executor()
                    .execute("UPDATE machine_enrollments SET state='revoked'")
                    .await
                    .map_err(StoreError::backend)?;
                let mut session_scope = CoreSessionScope::<ScopeHost, FixtureIo> {
                    inner: Store::reborrow(scope),
                    registrations: Default::default(),
                    edits: Default::default(),
                    effects: Vec::new(),
                    transport: Vec::new(),
                    wake_at: None,
                    marker: PhantomData,
                };
                let operation: OperationRequest<board_sync_protocol::BoardConsumerProtocol> =
                    serde_json::from_value(body).map_err(StoreError::backend)?;
                let session = SessionKey {
                    peer: PEER.to_owned().try_into().map_err(StoreError::backend)?,
                    session_id: SessionId::new(durable_steps::Uuid::now_v7())
                        .map_err(StoreError::backend)?,
                };
                session_scope
                    .stage_frame_effects(
                        vec![SessionEffect::Route {
                            session,
                            frame: FrameId::new(durable_steps::Uuid::now_v7())
                                .map_err(StoreError::backend)?,
                            operation,
                        }],
                        ConnectionId::new(durable_steps::Uuid::now_v7())
                            .map_err(StoreError::backend)?,
                        SERVICE.to_owned().try_into().map_err(StoreError::backend)?,
                        NOW,
                        FrameId::new(durable_steps::Uuid::now_v7()).map_err(StoreError::backend)?,
                    )
                    .await?;
                let [crate::session_store::SessionTransportEffect::Frame { bytes, .. }] =
                    session_scope.transport.as_slice()
                else {
                    return Err(StoreError::backend(
                        "revoked caller selected shared work instead of one refusal",
                    ));
                };
                let frame: Value = serde_json::from_slice(bytes).map_err(StoreError::backend)?;
                if frame.pointer("/value/body") != Some(&expected) {
                    return Err(StoreError::backend(format!(
                        "wrong enrollment refusal: {frame}"
                    )));
                }
                if session_scope.wake_at.is_some() {
                    return Err(StoreError::backend("refusal scheduled shared work"));
                }
                Ok(())
            })
        })
        .await?;
    fixture.close().await
}
#[tokio::test]
async fn revoked_fleet_request_selects_only_its_admission_refusal() -> Result<(), TestError> {
    selection(json!({"kind":"consumer","value":{"kind":"fleet","value":{"requestId":PROJECT,"operation":{}}}}),json!({"kind":"consumer","value":{"kind":"fleet","value":{"requestId":PROJECT,"decision":{"kind":"callerNotEnrolled"}}}})).await
}
#[tokio::test]
async fn revoked_archive_seen_selects_only_its_admission_refusal() -> Result<(), TestError> {
    selection(json!({"kind":"consumer","value":{"kind":"archiveSeen","value":{"board":PROJECT,"project":PROJECT,"position":{"epoch":1,"sequence":0}}}}),json!({"kind":"consumer","value":{"kind":"archiveSeen","value":{"kind":"refused","value":{"kind":"callerNotEnrolled"}}}})).await
}
#[tokio::test]
async fn revoked_forward_selects_no_pending_row_or_route() -> Result<(), TestError> {
    selection(json!({"kind":"consumer","value":{"kind":"forwardedPost","value":{"project":PROJECT,"post":{"kind":"threadPost","body":{}}}}}),json!({"kind":"consumer","value":{"kind":"forwardedPost","value":{"kind":"callerNotEnrolled"}}})).await
}
#[tokio::test]
async fn revoked_project_settle_selects_no_negative_custody() -> Result<(), TestError> {
    selection(json!({"kind":"delivery","value":{"kind":"settle","value":{"call":{"operation":PROJECT,"step":"settle"},"submitter":{"kind":"project","value":PROJECT},"mailbox":PEER,"item":PROJECT,"digest":"ab".repeat(32)}}}),json!({"kind":"delivery","value":{"kind":"deliveryUnavailable","value":{"reason":{"kind":"senderNotEnrolled"}}}})).await
}
