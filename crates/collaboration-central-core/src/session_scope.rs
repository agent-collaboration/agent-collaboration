//! A shortened shared SQL/journal scope with transaction-local transport edits.

use crate::session_store::PendingWelcome;
use crate::session_store::SessionTransportEffect;
use crate::{
    ObjectHost, PlatformIo, RegistrationEdits, RegistrationSnapshot,
    session_store::CoreSessionStore,
};
use durable_steps::{
    Admitted, AtomicStore, DriveLease, JournalScope, OperationId, ReborrowScope, RequestKey,
    StartRefusal, StoreError, SyncTime, Task, Uuid,
    storage::{JournalStorage, OperationOutcome, OperationRecord, StepRecord, TransactionScope},
};
use fleet_sync_protocol::{ConnectionId, PeerId, SessionId};
use fleet_sync_session::{
    ConnectionRegistration, PublishSchemaSupport, PublishSchemaSupportInput, SessionScope,
    schema_support_request_key,
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::marker::PhantomData;
mod operations;

pub(crate) struct CoreSessionScope<'transaction, Host: ObjectHost, Io: PlatformIo>
where
    Host::Store: ReborrowScope,
{
    pub inner: <Host::Store as AtomicStore>::Scope<'transaction>,
    pub registrations: RegistrationSnapshot,
    pub edits: RegistrationEdits,
    pub effects: Vec<PendingWelcome>,
    pub transport: Vec<SessionTransportEffect>,
    pub wake_at: Option<SyncTime>,
    pub marker: PhantomData<fn() -> Io>,
}

impl<'transaction, Host: ObjectHost, Io: PlatformIo> TransactionScope
    for CoreSessionScope<'transaction, Host, Io>
where
    Host::Store: ReborrowScope,
    <Host::Store as AtomicStore>::Scope<'transaction>: TransactionScope<Database = Host::Database>,
{
    type Database = Host::Database;
    type Executor<'query>
        = <<Host::Store as AtomicStore>::Scope<'transaction> as TransactionScope>::Executor<'query>
    where
        Self: 'query;
    fn executor(&mut self) -> Self::Executor<'_> {
        self.inner.executor()
    }
}

impl<Host: ObjectHost, Io: PlatformIo> SessionScope for CoreSessionScope<'_, Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    fn registration(&self, connection: &ConnectionId) -> Option<ConnectionRegistration> {
        self.edits
            .get(connection)
            .or_else(|| self.registrations.get(connection))
            .copied()
    }
    fn set_registration(
        &mut self,
        connection: &ConnectionId,
        registration: ConnectionRegistration,
    ) {
        self.edits.insert(*connection, registration);
    }
    async fn publish_schema_support(
        &mut self,
        input: PublishSchemaSupportInput,
        now: SyncTime,
    ) -> Result<(), fleet_sync_session::StoreError> {
        let key = schema_support_request_key(&input.report).map_err(session_error)?;
        self.admit::<PublishSchemaSupport<CoreSessionStore<Host, Io>, Host::Database>>(
            &key, &input,
        )
        .await
        .map_err(session_error)?;
        self.ensure_wake(now).await.map_err(session_error)
    }
    fn welcome_after_commit(
        &mut self,
        connection: ConnectionId,
        _peer: PeerId,
        session_id: SessionId,
    ) {
        self.effects.push(PendingWelcome {
            connection,
            session_id,
        });
    }
}
fn session_error(error: impl std::fmt::Display) -> fleet_sync_session::StoreError {
    fleet_sync_session::StoreError::Unavailable {
        message: error.to_string(),
    }
}

impl<Host: ObjectHost, Io: PlatformIo> CoreSessionScope<'_, Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    /// Every operation the removed board, replication and mail surfaces served is
    /// refused with the existing designed operation refusal.
    pub(crate) fn stage_frame_effects(
        &mut self,
        effects: Vec<fleet_sync_session::SessionEffect<board_sync_protocol::BoardConsumerProtocol>>,
        connection: ConnectionId,
    ) -> Result<(), StoreError> {
        use fleet_sync_protocol::OperationAnswer;
        use fleet_sync_session::SessionEffect;
        for effect in effects {
            match effect {
                SessionEffect::Route { frame, .. } => self.stage_operation_answer(
                    connection,
                    frame,
                    OperationAnswer::InvalidShape {
                        field: "operation".into(),
                    },
                )?,
                SessionEffect::RouteAnswer { .. } => {}
                other => self.stage_transport(vec![other])?,
            }
        }
        Ok(())
    }
}

impl<Host: ObjectHost, Io: PlatformIo> JournalScope for CoreSessionScope<'_, Host, Io>
where
    Host::Store: ReborrowScope,
{
    async fn running_request_keys(&mut self, kind: &str) -> Result<Vec<RequestKey>, StoreError> {
        self.inner.running_request_keys(kind).await
    }

    async fn admit<SharedTask: Task>(
        &mut self,
        key: &RequestKey,
        input: &SharedTask::Input,
    ) -> Result<Admitted, StartRefusal> {
        let admitted = self.inner.admit::<SharedTask>(key, input).await?;
        // Native journal admission records work but does not arm a timer.
        // Capture its deadline in the session commit, so the host wakes only
        // after the transaction and registration publication succeed.
        if let Some(at) = self.inner.earliest().await? {
            self.ensure_wake(at).await?;
        }
        Ok(admitted)
    }
    async fn ensure_wake(&mut self, at: SyncTime) -> Result<(), StoreError> {
        self.inner.ensure_wake(at).await?;
        self.wake_at = Some(self.wake_at.map_or(at, |current| current.min(at)));
        Ok(())
    }
}

impl<Host: ObjectHost, Io: PlatformIo> JournalStorage for CoreSessionScope<'_, Host, Io>
where
    Host::Store: ReborrowScope,
{
    async fn operation(&mut self, id: OperationId) -> Result<Option<OperationRecord>, StoreError> {
        self.inner.operation(id).await
    }
    async fn due(&mut self, now: SyncTime) -> Result<Vec<OperationId>, StoreError> {
        self.inner.due(now).await
    }
    async fn claim(
        &mut self,
        id: OperationId,
        now: SyncTime,
        lease: DriveLease,
    ) -> Result<Option<OperationRecord>, StoreError> {
        self.inner.claim(id, now, lease).await
    }
    async fn check_token(&mut self, id: OperationId, token: Uuid) -> Result<(), StoreError> {
        self.inner.check_token(id, token).await
    }
    async fn retry(
        &mut self,
        id: OperationId,
        token: Uuid,
        next_attempt_at: SyncTime,
        attempt: u32,
    ) -> Result<(), StoreError> {
        self.inner.retry(id, token, next_attempt_at, attempt).await
    }
    async fn finish(
        &mut self,
        id: OperationId,
        token: Uuid,
        outcome: OperationOutcome,
        at: SyncTime,
        keep_result: bool,
    ) -> Result<(), StoreError> {
        self.inner.finish(id, token, outcome, at, keep_result).await
    }
    async fn step(
        &mut self,
        id: OperationId,
        position: u64,
    ) -> Result<Option<StepRecord>, StoreError> {
        self.inner.step(id, position).await
    }
    async fn save_step(&mut self, step: StepRecord, token: Uuid) -> Result<(), StoreError> {
        self.inner.save_step(step, token).await
    }
    async fn step_count(&mut self, id: OperationId) -> Result<u64, StoreError> {
        self.inner.step_count(id).await
    }
    async fn earliest(&mut self) -> Result<Option<SyncTime>, StoreError> {
        self.inner.earliest().await
    }
}

impl<Host: ObjectHost, Io: PlatformIo> CoreSessionScope<'_, Host, Io>
where
    Host::Store: ReborrowScope,
{
    pub(crate) fn stage_transport(
        &mut self,
        effects: Vec<fleet_sync_session::SessionEffect<board_sync_protocol::BoardConsumerProtocol>>,
    ) -> Result<(), StoreError> {
        for effect in effects {
            match effect {
                fleet_sync_session::SessionEffect::Welcome {
                    connection,
                    session_id,
                    ..
                } => self.effects.push(PendingWelcome {
                    connection,
                    session_id,
                }),
                fleet_sync_session::SessionEffect::Send { connection, frame } => {
                    self.transport.push(SessionTransportEffect::Frame {
                        connection,
                        bytes: serde_json::to_vec(&frame)
                            .map_err(StoreError::backend)?
                            .into(),
                    })
                }
                fleet_sync_session::SessionEffect::Close { connection, reason } => self
                    .transport
                    .push(SessionTransportEffect::Close { connection, reason }),
                fleet_sync_session::SessionEffect::Route { .. } => {
                    return Err(StoreError::Aborted {
                        reason: "a non-frame session step produced an operation".into(),
                    });
                }
                fleet_sync_session::SessionEffect::RouteAnswer { .. } => {}
            }
        }
        Ok(())
    }
}
