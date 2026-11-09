//! The one platform contract each shell implements for a central object.
//!
//! `ObjectPlatform` owns what an object has: its store, its one alarm, its
//! sockets and the local section. Its `Io` is how the object acts in time:
//! clock, ids, event-scoped deadlines and calls to other objects. They stay two
//! traits because the store's durable-steps host holds the `Io` (CoreHost<Io>);
//! one struct owning both would close an Arc cycle and the object would never drop.

use crate::{AdmissionOrigin, ObjectAddress};
use durable_steps::{AtomicStore, CallFailure};
use fleet_protocol::{CloseReason, ConnectionId};
use fleet_session::{Bytes, ConnectionRegistration};
use runtime_primitives::{Clock, IdSource, WakeScheduler};
use sqlx_core::database::Database;
use std::collections::BTreeMap;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

pub trait ObjectIo: Clock + IdSource + WakeScheduler + Send + Sync + 'static {
    fn event_deadline(&self, remaining: Duration) -> impl Future<Output = ()> + Send;
    #[cfg(feature = "conformance")]
    fn before_local_call(
        &self,
        _request: &[u8],
    ) -> impl Future<Output = Result<(), CallFailure>> + Send {
        std::future::ready(Ok(()))
    }
    #[cfg(feature = "conformance")]
    fn drop_local_reply(&self, _request: &[u8]) -> Result<bool, CallFailure> {
        Ok(false)
    }
    fn object_call(
        &self,
        destination: ObjectAddress,
        body: Vec<u8>,
    ) -> impl Future<Output = Result<Vec<u8>, CallFailure>> + Send;
    /// Call another object, bounded by an event-scoped deadline. The deadline is
    /// dropped with the call, so no timer outlives the event that made it.
    fn call(
        &self,
        destination: ObjectAddress,
        body: Vec<u8>,
        deadline: Duration,
    ) -> impl Future<Output = Result<Vec<u8>, ObjectCallFailure>> + Send {
        let answer = self.object_call(destination, body);
        let expiry = self.event_deadline(deadline);
        async move {
            futures_util::pin_mut!(answer, expiry);
            match futures_util::future::select(answer, expiry).await {
                futures_util::future::Either::Left((answer, _)) => {
                    answer.map_err(ObjectCallFailure::Call)
                }
                futures_util::future::Either::Right(((), _)) => {
                    Err(ObjectCallFailure::DeadlineExceeded)
                }
            }
        }
    }
}

pub trait ObjectPlatform: Send + Sync + 'static {
    type Database: Database;
    type Store: AtomicStore;
    /// The object's one alarm: lowered inside units, set exactly by the recompute.
    type Alarm: crate::ObjectAlarm;
    /// Clock, ids, event deadlines and object calls for this object.
    type Io: crate::ObjectIo;

    fn store(&self) -> &Arc<Self::Store>;
    fn io(&self) -> &Arc<Self::Io>;
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
    fn alarm(&self) -> &Self::Alarm;
    /// Run `section` exclusively. A platform may poll it as its own task
    /// (workerd's `blockConcurrencyWhile` does); the section carries the event
    /// that built it, so that is safe.
    fn with_local_section<Output: Send + 'static>(
        &self,
        section: LocalSection<Output>,
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
    ) -> impl Future<Output = Result<fleet_session::ConnectionBinding, SocketFailure>> + Send;
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

/// Work for a platform's local section, carrying the event that built it.
///
/// The only way to build one ([`LocalSection::new`]) captures the running
/// event's scopes (what it owes, and in conformance builds its trace),
/// restored around every poll. So no caller can hand a platform a section that
/// would lose its event when the platform polls it as its own task.
pub struct LocalSection<Output>(Pin<Box<dyn Future<Output = Output> + Send>>);

impl<Output> LocalSection<Output> {
    /// Capture the running event's scopes for `work`.
    pub fn new(work: impl Future<Output = Output> + Send + 'static) -> Self {
        Self(Box::pin(crate::event_scope_store::carried(work)))
    }
}

impl<Output> Future for LocalSection<Output> {
    type Output = Output;

    fn poll(self: Pin<&mut Self>, context: &mut std::task::Context<'_>) -> std::task::Poll<Output> {
        self.get_mut().0.as_mut().poll(context)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LocalSectionFailure {
    #[error("local critical section was interrupted")]
    Interrupted,
}

/// Why a deadline-bounded object call returned no answer.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ObjectCallFailure {
    /// The event's deadline passed first; the call may still have reached its target.
    #[error("object call deadline exceeded")]
    DeadlineExceeded,
    #[error(transparent)]
    Call(CallFailure),
}

impl ObjectCallFailure {
    /// A durable-steps caller learns only that the answer is lost: the call may
    /// have reached its target, so the engine must not treat it as unsent.
    pub fn into_call_failure(self) -> CallFailure {
        match self {
            Self::DeadlineExceeded => CallFailure::AnswerLost,
            Self::Call(failure) => failure,
        }
    }
}

#[cfg(test)]
#[path = "object_platform_test.rs"]
mod tests;
