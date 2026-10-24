//! One physical mailbox owns lifecycle receipts and library mail state together.

use crate::{
    CallerKind, CoreHost, MailboxKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    mailbox_lifecycle::{ClosePeerMailbox, OpenPeerMailbox},
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, DriveReport, Engine, JournalScope,
    NotSentReason, OperationContext, OperationFailure, RequestKey, StoreError, Task,
    storage::{JournalStorage, OperationExecution, TransactionScope},
};
use fleet_sync_delivery::{DeliveryLimits, Mailbox};
use fleet_sync_protocol::{PeerId, ReceiverId};
use message_board::ServiceId;
use runtime_primitives::{Clock, WakeScheduler};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MailboxLifecycleInput {
    peer: PeerId,
}

#[derive(Clone, Copy)]
enum MailboxChange {
    Open,
    Close,
}

struct AdmitMailboxLifecycle {
    key: RequestKey,
    peer: PeerId,
    change: MailboxChange,
    now: runtime_primitives::SyncTime,
}

struct ActivateMailbox;

impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for AdmitMailboxLifecycle
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = bool;
    type Effect = ActivateMailbox;

    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let input = MailboxLifecycleInput { peer: self.peer };
            let admitted = match self.change {
                MailboxChange::Close => {
                    scope
                        .admit::<CloseMailboxTask<Store, Driver>>(&self.key, &input)
                        .await
                }
                MailboxChange::Open => {
                    scope
                        .admit::<OpenMailboxTask<Store, Driver>>(&self.key, &input)
                        .await
                }
            }
            .map_err(StoreError::backend)?;
            let operation = scope
                .operation(admitted.id())
                .await?
                .ok_or(StoreError::MissingOperation { id: admitted.id() })?;
            let completed = match operation.execution {
                OperationExecution::Completed { result, .. } => {
                    let (): () = serde_json::from_slice(&result).map_err(StoreError::backend)?;
                    true
                }
                OperationExecution::Running { .. } => {
                    scope.ensure_wake(self.now).await?;
                    false
                }
                OperationExecution::Failed { .. } => {
                    return Err(StoreError::Aborted {
                        reason: "mailbox lifecycle operation failed".into(),
                    });
                }
            };
            let effects = if completed {
                Vec::new()
            } else {
                vec![ActivateMailbox]
            };
            Ok((completed, effects))
        })
    }
}

struct CloseMailboxTask<Store, Driver> {
    limits: DeliveryLimits,
    marker: PhantomData<fn() -> (Store, Driver)>,
}
impl<Store: AtomicStore, Driver: Database> Task for CloseMailboxTask<Store, Driver>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    const KIND: &'static str = "central.close-mailbox";
    const VERSION: u32 = 1;
    type Store = Store;
    type Input = MailboxLifecycleInput;
    type Output = ();

    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        context: &mut Context,
        input: &MailboxLifecycleInput,
    ) -> Result<(), OperationFailure> {
        let peer = input.peer;
        let limits = self.limits;
        context
            .local(
                durable_steps::StepName::new("close_mailbox").map_err(|error| {
                    OperationFailure::InvalidPayload {
                        message: error.to_string(),
                    }
                })?,
                move |scope| {
                    Box::pin(async move {
                        Mailbox::<_, BoardDeliveryTypes>::new(scope, peer, limits)
                            .close()
                            .await
                            .map_err(StoreError::backend)?;
                        fleet_sync_delivery::admit_publications::<
                            _,
                            BoardDeliveryTypes,
                            fleet_sync_delivery::PublishStatus<Store, Driver, BoardDeliveryTypes>,
                        >(scope)
                        .await
                        .map_err(fleet_sync_delivery::delivery_rollback)?;
                        Ok(())
                    })
                },
            )
            .await?;
        Ok(())
    }
}

struct OpenMailboxTask<Store, Driver> {
    limits: DeliveryLimits,
    marker: PhantomData<fn() -> (Store, Driver)>,
}
impl<Store: AtomicStore, Driver: Database> Task for OpenMailboxTask<Store, Driver>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    const KIND: &'static str = "central.open-mailbox";
    const VERSION: u32 = 1;
    type Store = Store;
    type Input = MailboxLifecycleInput;
    type Output = ();

    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        context: &mut Context,
        input: &MailboxLifecycleInput,
    ) -> Result<(), OperationFailure> {
        let peer = input.peer;
        let limits = self.limits;
        context
            .local(
                durable_steps::StepName::new("open_mailbox").map_err(|error| {
                    OperationFailure::InvalidPayload {
                        message: error.to_string(),
                    }
                })?,
                move |scope| {
                    Box::pin(async move {
                        Mailbox::<_, BoardDeliveryTypes>::new(scope, peer, limits)
                            .open()
                            .await
                            .map_err(StoreError::backend)?;
                        Ok(())
                    })
                },
            )
            .await?;
        Ok(())
    }
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<MailboxKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub fn mailbox(
        host: Arc<Host>,
        io: Io,
        receiver: ReceiverId,
        installation: ServiceId,
        limits: DeliveryLimits,
    ) -> Result<Self, StoreError> {
        let shared_host = Arc::new(CoreHost::mailbox(io, receiver, installation.clone()));
        let mut engine = Engine::new(host.store().clone(), shared_host.clone());
        engine.register(CloseMailboxTask::<Host::Store, Host::Database> {
            limits,
            marker: PhantomData,
        })?;
        engine.register(OpenMailboxTask::<Host::Store, Host::Database> {
            limits,
            marker: PhantomData,
        })?;
        engine.register(
            crate::machine_notice_outbox::MailboxNoticeTask::<Host::Store>(PhantomData),
        )?;
        engine.register(fleet_sync_delivery::PublishStatus::<
            Host::Store,
            Host::Database,
            BoardDeliveryTypes,
        >::new())?;
        Ok(Self {
            work: ObjectWork::Mailbox {
                engine,
                host,
                shared_host,
                installation,
                limits,
                health: Arc::default(),
            },
            face: PhantomData,
        })
    }

    pub async fn initialize(&self) -> Result<usize, StoreError> {
        let ObjectWork::Mailbox {
            host, shared_host, ..
        } = &self.work
        else {
            return Err(wrong_face());
        };
        let applied = crate::ObjectBootstrap::new(host.clone(), shared_host.now())
            .initialize(crate::EngineSchema::Mailbox)
            .await?;
        self.maintain_deadlines().await?;
        Ok(applied)
    }

    pub async fn on_alarm(&self) -> Result<DriveReport, StoreError> {
        let ObjectWork::Mailbox { engine, health, .. } = &self.work else {
            return Err(wrong_face());
        };
        if health.failure()?.is_some() {
            return Ok(DriveReport::default());
        }
        self.maintain_deadlines().await?;
        engine.drive_due().await
    }

    pub async fn on_call(&self, bytes: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        let ObjectWork::Mailbox {
            host,
            shared_host,
            installation,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        let invocation = ObjectCallEnvelope::decode(
            &bytes,
            ObjectAddress::Mailbox(installation.clone()),
            CallerKind::Mailbox,
        )?;
        if invocation.kind == crate::mail_submit_calls::MailboxSubmit::KIND {
            return self.mailbox_submit_call(invocation).await;
        }
        if invocation.kind == crate::mailbox_requests::MailboxRequest::KIND {
            return self.mailbox_request_call(invocation).await;
        }
        if !matches!(invocation.authority, CallAuthority::Object)
            || invocation.source != ObjectAddress::Registry
            || invocation.source_face != CallerKind::Registry
        {
            return Err(refused());
        }
        let (change, peer, prefix) = match invocation.kind.as_str() {
            ClosePeerMailbox::KIND => {
                let target: ClosePeerMailbox =
                    serde_json::from_value(invocation.target).map_err(encode_refusal)?;
                (MailboxChange::Close, target.peer, "central-close-mailbox")
            }
            OpenPeerMailbox::KIND => {
                let target: OpenPeerMailbox =
                    serde_json::from_value(invocation.target).map_err(encode_refusal)?;
                (MailboxChange::Open, target.peer, "central-open-mailbox")
            }
            _ => return Err(refused()),
        };
        let (): () = serde_json::from_value(invocation.request).map_err(encode_refusal)?;
        let key = RequestKey::new(format!(
            "{prefix}:{}:{}",
            String::from(invocation.identity.operation),
            invocation.identity.step.as_str()
        ))
        .map_err(encode_refusal)?;
        let now = shared_host.now();
        let (completed, committed) = StepDriver::new(host.clone())
            .run(AdmitMailboxLifecycle {
                key,
                peer,
                change,
                now,
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        for ActivateMailbox in committed.into_effects() {
            host.wake()
                .wake_at(now)
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
        }
        if completed {
            serde_json::to_vec(&()).map_err(|_| CallFailure::AnswerLost)
        } else {
            // Only the saved completion acknowledges the call. Alarm execution
            // never keeps the incoming call or another object's exclusion open.
            Err(CallFailure::AnswerLost)
        }
    }
}

fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
fn wrong_face() -> StoreError {
    StoreError::Aborted {
        reason: "mailbox runtime has a different face".into(),
    }
}
