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
    ObjectHost, PlatformIo, RegistrationEdits, RegistrationSnapshot,
    effect_driver::EffectDriver,
    session_scope::CoreSessionScope,
    step_driver::{StepCallback, StepDriver},
};
use durable_steps::{AtomicStore, BoxFuture, ReborrowScope, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{ConnectionId, SessionId};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

pub(crate) struct PendingWelcome {
    pub connection: ConnectionId,
    pub session_id: SessionId,
}

pub(crate) enum SessionTransportEffect {
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
    pub(crate) fn new(host: Arc<Host>) -> Self {
        Self {
            effects: EffectDriver::new(host.clone()),
            host,
        }
    }
}

// Bound wasm monomorphization by erasing callbacks at the existing store boundary.
type SessionTransactionCallback<Host, Io, Output> =
    dyn for<'scope, 'transaction> FnOnce(
            &'scope mut CoreSessionScope<'transaction, Host, Io>,
        ) -> BoxFuture<'scope, Result<Output, StoreError>>
        + Send;

struct SessionTransaction<Host: ObjectHost, Io: PlatformIo, Output>
where
    Host::Store: ReborrowScope,
{
    registrations: RegistrationSnapshot,
    callback: Box<SessionTransactionCallback<Host, Io, Output>>,
    marker: PhantomData<fn() -> (Host, Io)>,
}
impl<Host: ObjectHost, Io: PlatformIo, Output> StepCallback<Host::Store, Host::Database>
    for SessionTransaction<Host, Io, Output>
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
    Output: Send + 'static,
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
            let output = (self.callback)(&mut scope).await?;
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
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
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
                let result = StepDriver::new(host)
                    .run(SessionTransaction::<Host, Io, Output> {
                        registrations,
                        callback: Box::new(run),
                        marker: PhantomData,
                    })
                    .await;
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
