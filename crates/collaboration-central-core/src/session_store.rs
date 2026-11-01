//! Adapt shared session tasks through central's one transaction/effect boundary.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::arithmetic_side_effects,
    clippy::unimplemented,
    clippy::unreachable
)]

use crate::{
    CoreHost, ObjectHost, PlatformIo, RegistrationEdits, RegistrationSnapshot,
    effect_driver::EffectDriver,
    session_scope::CoreSessionScope,
    step_driver::{StepCallback, StepDriver},
};
use durable_steps::{AtomicStore, BoxFuture, ReborrowScope, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{ConnectionId, PeerId, SessionId};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

pub(crate) struct PendingWelcome {
    pub connection: ConnectionId,
    pub peer: PeerId,
    pub session_id: SessionId,
}

pub(crate) enum SessionTransportEffect {
    InstallationRead {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        request: crate::installation_peer_reads::InstallationReadRequest,
    },
    ForwardRoute {
        pending: crate::forward_store::PendingForward,
        post: board_sync_protocol::ForwardedPost,
    },
    ForwardDelivery {
        installation: message_board::ServiceId,
        request: crate::forward_calls::ForwardDeliveryRequest,
        last_seen: Option<SyncTime>,
    },
    ForwardReply {
        installation: message_board::ServiceId,
        request: crate::forward_calls::ForwardAnswerRequest,
    },
    ForwardFrame {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        bytes: fleet_sync_session::Bytes,
    },
    FleetRead {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        request: crate::fleet_read_calls::FleetReadRequest,
    },
    MailboxRequest {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        request: Box<fleet_sync_protocol::DeliveryRequest<board_sync_protocol::BoardDeliveryTypes>>,
    },
    FleetRequest {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        request: board_sync_protocol::FleetRequestEnvelope,
    },
    AutomationRequest {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        request: board_sync_protocol::BoardPayload,
    },
    ArchiveReport {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        report: board_sync_protocol::ArchiveSeen,
    },
    MailSubmit {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        item: Box<fleet_sync_protocol::DeliveryEnvelope<board_sync_protocol::BoardDeliveryTypes>>,
        origin_epoch: Option<fleet_sync_protocol::Epoch>,
    },
    FleetEffects {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        request: message_board::RequestEffectsRequest,
    },
    CopyWrite {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        database: fleet_sync_protocol::DatabaseId,
        request: Box<crate::copy_write_calls::WriterCopyRequest>,
    },
    CopyRead {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        database: fleet_sync_protocol::DatabaseId,
        request: crate::copy_read_calls::CopyReadRequest,
    },
    PeerAnswer {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        answer: fleet_sync_protocol::ClientAnswer<board_sync_protocol::BoardConsumerProtocol>,
    },
    LeaseRequest {
        connection: ConnectionId,
        session: fleet_sync_session::SessionKey,
        frame: fleet_sync_protocol::FrameId,
        request: fleet_sync_protocol::LeaseRequest,
    },
    Frame {
        connection: ConnectionId,
        bytes: fleet_sync_session::Bytes,
    },
    Close {
        connection: ConnectionId,
        reason: fleet_sync_protocol::CloseReason,
    },
}

pub(crate) struct SessionCommit {
    pub registrations: RegistrationEdits,
    pub effects: Vec<PendingWelcome>,
    pub transport: Vec<SessionTransportEffect>,
    pub wake_at: Option<SyncTime>,
}

pub(crate) struct CoreSessionStore<Host: ObjectHost, Io: PlatformIo> {
    host: Arc<Host>,
    effects: EffectDriver<Host, Io>,
}
impl<Host: ObjectHost, Io: PlatformIo> CoreSessionStore<Host, Io> {
    pub(crate) fn new(
        host: Arc<Host>,
        shared_host: Arc<CoreHost<Io>>,
        replies: Arc<crate::holder_reply_hub::HolderReplyHub>,
    ) -> Self {
        Self {
            effects: EffectDriver::new(host.clone(), shared_host, replies),
            host,
        }
    }
}

struct SessionTransaction<Host: ObjectHost, Io: PlatformIo, Callback> {
    registrations: RegistrationSnapshot,
    callback: Callback,
    #[cfg(feature = "conformance")]
    finalize_fault: Option<crate::SessionFinalizeFault>,
    marker: PhantomData<fn() -> (Host, Io)>,
}
impl<Host: ObjectHost, Io: PlatformIo, Output, Callback> StepCallback<Host::Store, Host::Database>
    for SessionTransaction<Host, Io, Callback>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
    Output: Send + 'static,
    Callback: for<'scope, 'transaction> FnOnce(
            &'scope mut CoreSessionScope<'transaction, Host, Io>,
        ) -> BoxFuture<'scope, Result<Output, StoreError>>
        + Send
        + 'static,
{
    type Output = Output;
    type Effect = SessionCommit;
    fn run<'scope, 'transaction>(
        self,
        inner: &'scope mut <Host::Store as AtomicStore>::Scope<'transaction>,
    ) -> BoxFuture<'scope, Result<(Output, Vec<SessionCommit>), StoreError>> {
        Box::pin(async move {
            let mut scope = CoreSessionScope::<Host, Io> {
                inner: Host::Store::reborrow(inner),
                registrations: self.registrations,
                edits: Default::default(),
                effects: Vec::new(),
                transport: Vec::new(),
                wake_at: None,
                marker: PhantomData,
            };
            #[cfg(feature = "conformance")]
            let before = if self.finalize_fault.is_some() {
                Some(crate::session_finalize_fault::changes(&mut scope).await?)
            } else {
                None
            };
            let output = (self.callback)(&mut scope).await?;
            #[cfg(feature = "conformance")]
            if let (Some(fault), Some(before)) = (self.finalize_fault, before)
                && (!scope.effects.is_empty()
                    || scope.edits.values().any(|registration| {
                        matches!(
                            registration,
                            fleet_sync_session::ConnectionRegistration::Attached { .. }
                        )
                    }))
            {
                crate::session_finalize_fault::fail_after_writes(&mut scope, fault, before).await?;
            }
            Ok((
                output,
                vec![SessionCommit {
                    registrations: scope.edits,
                    effects: scope.effects,
                    transport: scope.transport,
                    wake_at: scope.wake_at,
                }],
            ))
        })
    }
}

impl<Host: ObjectHost, Io: PlatformIo> AtomicStore for CoreSessionStore<Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    type Scope<'transaction> = CoreSessionScope<'transaction, Host, Io>;
    async fn transaction<Output, Callback>(&self, run: Callback) -> Result<Output, StoreError>
    where
        Output: Send + 'static,
        Callback: for<'scope, 'transaction> FnOnce(
                &'scope mut Self::Scope<'transaction>,
            )
                -> BoxFuture<'scope, Result<Output, StoreError>>
            + Send
            + 'static,
    {
        let host = self.host.clone();
        let effects = self.effects.clone();
        let local = self
            .host
            .with_local_section(async move {
                let registrations = host.registrations().await.map_err(StoreError::backend)?;
                #[cfg(feature = "conformance")]
                let finalize_fault = host.session_finalize_fault()?;
                #[cfg(feature = "conformance")]
                let observation_host = host.clone();
                let result = StepDriver::new(host)
                    .run(SessionTransaction::<Host, Io, _> {
                        registrations,
                        callback: run,
                        #[cfg(feature = "conformance")]
                        finalize_fault,
                        marker: PhantomData,
                    })
                    .await;
                #[cfg(feature = "conformance")]
                if let Err(error) = &result
                    && let Some(failure) =
                        crate::session_finalize_fault::SessionFinalizeFailure::from_store_error(
                            error,
                        )
                {
                    let _ = observation_host.observe_session_finalize_failure(failure);
                }
                let (output, committed) = result?;
                let published = effects.publish_registrations(committed).await;
                Ok::<_, StoreError>((output, published))
            })
            .await
            .map_err(StoreError::backend)??;
        let (output, published) = local;
        self.effects.finish(published).await;
        Ok(output)
    }
}
