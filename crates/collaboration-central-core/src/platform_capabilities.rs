//! Capabilities supplied by the runtime for one central object.

use durable_steps::AtomicStore;
use fleet_sync_protocol::{CloseReason, ConnectionId};
use fleet_sync_session::{Bytes, ConnectionRegistration};
use runtime_primitives::WakeScheduler;
use sqlx_core::database::Database;
use std::collections::BTreeMap;
use std::{future::Future, sync::Arc};

use crate::{
    AccessJwk, AdmissionOrigin, CentralObservation, SerializedObjectCall, SignatureValidity,
};

pub struct RawJwks {
    pub body: Vec<u8>,
    pub cache_control: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyFetchFailure {
    Unreachable,
    BadResponse { status: u16 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifyFailure {
    UnsupportedKey,
    Crypto,
}

#[derive(Debug)]
pub enum CallOutcome<Reply> {
    Answered(Reply),
    Failed(durable_steps::CallFailure),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpgradeFailure {
    NotAnUpgrade,
    Refused,
}

pub trait CentralPlatform: Clone + Send + Sync + 'static {
    type Host: ObjectHost;
    fn clock(&self) -> &dyn runtime_primitives::Clock;
    fn ids(&self) -> &dyn runtime_primitives::IdSource;
    fn transport_object(
        &self,
        call: SerializedObjectCall,
    ) -> impl Future<Output = Result<Vec<u8>, durable_steps::CallFailure>> + Send;
    fn upgrade_session(
        &self,
        request: http::request::Parts,
        binding: fleet_sync_session::ConnectionBinding,
        admission_origin: AdmissionOrigin,
    ) -> impl Future<Output = Result<http::Response<axum::body::Body>, UpgradeFailure>> + Send;
    fn fetch_access_keys(&self) -> impl Future<Output = Result<RawJwks, KeyFetchFailure>> + Send;
    fn verify_rs256(
        &self,
        key: &AccessJwk,
        signed: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = Result<SignatureValidity, VerifyFailure>> + Send;
    fn observe(&self, observation: CentralObservation);
}

pub trait ObjectHost: Send + Sync + 'static {
    type Database: Database;
    type Store: AtomicStore;
    type Wake: WakeScheduler;

    fn store(&self) -> &Arc<Self::Store>;
    #[cfg(feature = "conformance")]
    fn session_finalize_fault(
        &self,
    ) -> Result<Option<crate::SessionFinalizeFault>, durable_steps::StoreError> {
        Ok(None)
    }
    #[cfg(feature = "conformance")]
    fn observe_session_finalize_failure(
        &self,
        _failure: crate::SessionFinalizeFailure,
    ) -> Result<(), durable_steps::StoreError> {
        Ok(())
    }
    #[cfg(feature = "conformance")]
    fn stale_drive_rejections(
        &self,
    ) -> Result<Vec<durable_steps::OperationId>, durable_steps::StoreError> {
        Err(durable_steps::StoreError::Aborted {
            reason: "drive-token observation is unavailable on this fixture host".into(),
        })
    }
    fn wake(&self) -> &Self::Wake;
    fn with_local_section<Output: Send + 'static>(
        &self,
        run: impl Future<Output = Output> + Send + 'static,
    ) -> impl Future<Output = Result<Output, LocalSectionFailure>> + Send;
    fn quarantine_connections(
        &self,
        connections: Vec<ConnectionId>,
    ) -> impl Future<Output = ()> + Send;
    fn send_frame(
        &self,
        to: ConnectionId,
        frame: Bytes,
    ) -> impl Future<Output = Result<(), SocketFailure>> + Send;
    fn close_connection(
        &self,
        to: ConnectionId,
        reason: CloseReason,
    ) -> impl Future<Output = Result<(), SocketFailure>> + Send;
    fn registrations(
        &self,
    ) -> impl Future<Output = Result<RegistrationSnapshot, SocketFailure>> + Send;
    fn admission_origin(
        &self,
        connection: ConnectionId,
    ) -> impl Future<Output = Result<AdmissionOrigin, SocketFailure>> + Send;
    fn admission_binding(
        &self,
        connection: ConnectionId,
    ) -> impl Future<Output = Result<fleet_sync_session::ConnectionBinding, SocketFailure>> + Send;
    fn publish_registrations(
        &self,
        edits: RegistrationEdits,
    ) -> impl Future<Output = Vec<(ConnectionId, SocketFailure)>> + Send;
}

pub type RegistrationSnapshot = BTreeMap<ConnectionId, ConnectionRegistration>;
pub type RegistrationEdits = BTreeMap<ConnectionId, ConnectionRegistration>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SocketFailure {
    #[error("connection is not present")]
    NotConnected,
    #[error("connection is closed")]
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LocalSectionFailure {
    #[error("local critical section was interrupted")]
    Interrupted,
}
