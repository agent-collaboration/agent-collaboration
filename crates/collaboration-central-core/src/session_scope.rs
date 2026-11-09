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
use fleet_protocol::{ConnectionId, PeerId, SessionId};
use fleet_session::{
    ConnectionRegistration, PublishSchemaSupport, PublishSchemaSupportInput, SessionScope,
    schema_support_request_key,
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::marker::PhantomData;
mod enrollment_gate;
mod fleet_reads;
mod forwards;
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
    ) -> Result<(), fleet_session::StoreError> {
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
        peer: PeerId,
        session_id: SessionId,
    ) {
        self.effects.push(PendingWelcome {
            connection,
            peer,
            session_id,
        });
    }
}
fn session_error(error: impl std::fmt::Display) -> fleet_session::StoreError {
    fleet_session::StoreError::Unavailable {
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
    /// Own-stream reads remain local to the admitted frame's transaction.
    pub(crate) async fn stage_frame_effects(
        &mut self,
        effects: Vec<fleet_session::SessionEffect<board_sync_protocol::BoardConsumerProtocol>>,
        connection: ConnectionId,
        installation: message_board::ServiceId,
        now: SyncTime,
        forward_frame: fleet_protocol::FrameId,
    ) -> Result<(), StoreError> {
        use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
        use fleet_protocol::{
            DeliveryAnswer, DeliveryRequest, OperationAnswer, OperationRequest, ServerFrame,
        };
        use fleet_session::SessionEffect;
        for effect in effects {
            if let SessionEffect::Route {
                session,
                frame,
                operation,
            } = &effect
                && let Some(answer) = self
                    .refuse_unenrolled_mutation(session.peer, operation, now)
                    .await?
            {
                self.stage_operation_answer(connection, *frame, answer)?;
                continue;
            }
            match effect {
                SessionEffect::RequestBatch { .. }
                | SessionEffect::AnswerBatch { .. }
                | SessionEffect::NoticesApplied { .. } => {
                    // D3 admits typed vocabulary only. D4 owns execution and replies.
                }
                SessionEffect::Route {
                    session,
                    frame,
                    operation: OperationRequest::Delivery(DeliveryRequest::Report { report, .. }),
                } => {
                    let limits = crate::fleet_outbox_limits().map_err(StoreError::backend)?;
                    let result = fleet_mail_delivery::Outbox::<_, BoardDeliveryTypes, _>::new(
                        self,
                        limits,
                        &crate::fleet_sender_binding::FleetSenderBinding,
                    )
                    .recipient_report::<crate::fleet_mail_tasks::FleetStatusTask<
                        crate::session_store::CoreSessionStore<Host, Io>,
                        Host::Database,
                    >>(session.peer, report, now)
                    .await
                    .map_err(fleet_mail_delivery::delivery_rollback)?;
                    self.stage_operation_answer(
                        connection,
                        frame,
                        OperationAnswer::Delivery(DeliveryAnswer::Report(result)),
                    )?;
                }
                SessionEffect::Route {
                    session,
                    frame,
                    operation:
                        OperationRequest::Delivery(DeliveryRequest::StatusAck { through, .. }),
                } => {
                    let result = fleet_mail_delivery::StatusStream::<_, BoardDeliveryTypes>::new(
                        self,
                        fleet_protocol::Submitter::Machine(session.peer),
                        installation.clone(),
                    )
                    .acknowledge(through, now)
                    .await
                    .map_err(StoreError::backend)?;
                    let bytes = serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Answer {
                        frame,
                        body: OperationAnswer::Delivery(DeliveryAnswer::StatusAck(result)),
                    })
                    .map_err(StoreError::backend)?;
                    self.transport.push(SessionTransportEffect::Frame {
                        connection,
                        bytes: bytes.into(),
                    });
                }
                SessionEffect::Route {
                    session,
                    frame,
                    operation:
                        OperationRequest::Delivery(DeliveryRequest::StatusFetch {
                            after, limit, ..
                        }),
                } => {
                    let answer =
                        match fleet_mail_delivery::StatusStream::<_, BoardDeliveryTypes>::new(
                            self,
                            fleet_protocol::Submitter::Machine(session.peer),
                            installation.clone(),
                        )
                        .fetch(
                            fleet_protocol::Submitter::Machine(session.peer),
                            after,
                            limit,
                        )
                        .await
                        {
                            Ok(result) => {
                                OperationAnswer::Delivery(DeliveryAnswer::StatusFetch(result))
                            }
                            Err(fleet_mail_delivery::DeliveryError::InvalidBatch) => {
                                OperationAnswer::InvalidShape {
                                    field: "after".into(),
                                }
                            }
                            Err(error) => return Err(StoreError::backend(error)),
                        };
                    let bytes = serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Answer {
                        frame,
                        body: answer,
                    })
                    .map_err(StoreError::backend)?;
                    self.transport.push(SessionTransportEffect::Frame {
                        connection,
                        bytes: bytes.into(),
                    });
                }
                SessionEffect::Route {
                    session,
                    frame,
                    operation,
                } => {
                    self.stage_operation(connection, session, frame, operation, now, forward_frame)
                        .await?;
                }
                SessionEffect::RouteAnswer {
                    session,
                    frame,
                    answer:
                        fleet_protocol::ClientAnswer::Consumer(
                            board_sync_protocol::BoardClientAnswer::ForwardedPost(reply),
                        ),
                } => {
                    self.stage_forward_answer(session, frame, reply, now)
                        .await?;
                }
                other => self.stage_transport(vec![other], Some(connection))?,
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
        effects: Vec<fleet_session::SessionEffect<board_sync_protocol::BoardConsumerProtocol>>,
        requesting_connection: Option<ConnectionId>,
    ) -> Result<(), StoreError> {
        let request_connection = || {
            requesting_connection.ok_or_else(|| StoreError::Aborted {
                reason: "a delivery-only session step cannot originate a request or answer".into(),
            })
        };
        for effect in effects {
            match effect {
                fleet_session::SessionEffect::RequestBatch { .. }
                | fleet_session::SessionEffect::AnswerBatch { .. }
                | fleet_session::SessionEffect::NoticesApplied { .. } => {
                    return Err(StoreError::Aborted {
                        reason: "wire admission requires the frame entry point".into(),
                    });
                }
                fleet_session::SessionEffect::Welcome {
                    connection,
                    peer,
                    session_id,
                } => self.effects.push(PendingWelcome {
                    connection,
                    peer,
                    session_id,
                }),
                fleet_session::SessionEffect::Send { connection, frame } => {
                    self.transport.push(SessionTransportEffect::Frame {
                        connection,
                        bytes: serde_json::to_vec(&frame)
                            .map_err(StoreError::backend)?
                            .into(),
                    })
                }
                fleet_session::SessionEffect::Close { connection, reason } => self
                    .transport
                    .push(SessionTransportEffect::Close { connection, reason }),
                fleet_session::SessionEffect::Route { .. } => {
                    return Err(StoreError::Aborted {
                        reason: "a non-frame session step produced an operation".into(),
                    });
                }
                fleet_session::SessionEffect::RouteAnswer {
                    session,
                    frame,
                    answer,
                } => {
                    self.transport.push(SessionTransportEffect::PeerAnswer {
                        connection: request_connection()?,
                        session,
                        frame,
                        answer,
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "session_enrollment_gate_test.rs"]
mod enrollment_gate_tests;
