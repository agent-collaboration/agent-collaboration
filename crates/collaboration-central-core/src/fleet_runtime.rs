//! The Fleet face owns central's nonreplicated application state.

use crate::{
    CallerKind, CoreHost, EngineSchema, FleetKind, ObjectAddress, ObjectBootstrap, ObjectHost,
    ObjectRuntime, PlatformIo, caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope, object_runtime::ObjectWork,
};
use durable_steps::{
    AtomicStore, CallFailure, JournalScope, NotSentReason, StoreError,
    storage::{JournalStorage, TransactionScope},
};
use runtime_primitives::WakeScheduler;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    Host::Store: durable_steps::ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'blob, Host::Database>
        + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub fn fleet(
        host: Arc<Host>,
        io: Io,
        config: fleet_sync_lease::RegistryConfig,
        delivery: crate::DeliveryConfig,
    ) -> Result<Self, StoreError> {
        let io = Arc::new(io);
        let shared_host = Arc::new(CoreHost::registry(
            io.clone(),
            config.authoritative_receiver,
            crate::registry_peer_lookup::bind(Arc::downgrade(&host)),
        ));
        let store = Arc::new(crate::session_store::CoreSessionStore::new(host.clone()));
        let mut engine = crate::shared_task_registry::registry_engine::<_, Host::Database, _>(
            store,
            shared_host.clone(),
        )?;
        engine.register(fleet_sync_session::PublishSchemaSupport::<
            crate::session_store::CoreSessionStore<Host, Io>,
            Host::Database,
        >::default())?;
        engine.register(crate::session_revocation::EndSessionsTask::<Host, Io>(
            PhantomData,
        ))?;
        let engine = crate::fleet_task_engine::own_engine(engine);
        let weak_host = Arc::downgrade(&host);
        let weak_shared = Arc::downgrade(&shared_host);
        let weak_engine = Arc::downgrade(&engine);
        let local_io = io.clone();
        shared_host.install_local_calls(Arc::new(move |bytes| {
            let host = weak_host.clone();
            let shared = weak_shared.clone();
            let engine = weak_engine.clone();
            let io = local_io.clone();
            Box::pin(async move {
                let runtime = ObjectRuntime::<FleetKind, Host, Io> {
                    work: ObjectWork::Fleet {
                        host: host.upgrade().ok_or(CallFailure::AnswerLost)?,
                        shared_host: shared.upgrade().ok_or(CallFailure::AnswerLost)?,
                        engine: engine.upgrade().ok_or(CallFailure::AnswerLost)?,
                        io,
                        delivery,
                    },
                    face: PhantomData,
                };
                runtime.on_call(bytes).await
            })
        }))?;
        Ok(Self {
            work: ObjectWork::Fleet {
                host,
                io,
                engine,
                shared_host,
                delivery,
            },
            face: PhantomData,
        })
    }
    pub async fn on_alarm(&self) -> Result<durable_steps::DriveReport, StoreError> {
        // Callers combine this alarm with socket work. Keep all its phases in
        // one heap-owned future rather than inflating every caller's frame.
        Box::pin(self.drive_fleet_alarm()).await
    }

    async fn drive_fleet_alarm(&self) -> Result<durable_steps::DriveReport, StoreError> {
        let ObjectWork::Fleet { engine, host, .. } = &self.work;
        let driven = engine.drive_due().await;
        let rearmed = host
            .store()
            .transaction(|scope| {
                Box::pin(async move {
                    if let Some(at) = scope.earliest().await? {
                        scope.ensure_wake(at).await?;
                    }
                    Ok(())
                })
            })
            .await;
        if let Err(failure) = &driven {
            eprintln!("Fleet job drive failed: {failure}");
        }
        if let Err(failure) = &rearmed {
            eprintln!("Fleet journal rearm failed: {failure}");
        }
        let report = driven?;
        rearmed?;
        Ok(report)
    }
    pub async fn initialize(&self) -> Result<usize, StoreError> {
        let ObjectWork::Fleet { host, io, .. } = &self.work;
        let bootstrap = ObjectBootstrap::new(host.clone(), io.clone());
        let mut applied = 0;
        for schema in [
            EngineSchema::Fleet,
            EngineSchema::Registry,
            EngineSchema::Session,
            EngineSchema::Delivery,
        ] {
            applied += bootstrap.initialize(schema).await?;
        }
        Ok(applied)
    }
    pub async fn on_call(&self, bytes: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        let envelope: ObjectCallEnvelope = serde_json::from_slice(&bytes).map_err(|_| refused())?;
        let answer = match envelope.destination_face {
            CallerKind::Registry => self.on_registry_call(bytes).await,
            CallerKind::Delivery => self.on_delivery_call(&bytes).await,
            _ => Err(refused()),
        };
        let ObjectWork::Fleet { host, .. } = &self.work;
        // The mutation saved its wake intent in its transaction. Only after it
        // settles do we arm the platform timer.
        let earliest = host
            .store()
            .transaction(|scope| Box::pin(async move { scope.earliest().await }))
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        if let Some(at) = earliest {
            host.wake()
                .wake_at(at)
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
        }
        answer
    }

    /// Only Fleet registry revocation reaches the Delivery face; it ends the peer sessions.
    async fn on_delivery_call(&self, bytes: &[u8]) -> Result<Vec<u8>, CallFailure> {
        let invocation =
            ObjectCallEnvelope::decode(bytes, ObjectAddress::Fleet, CallerKind::Delivery)?;
        if !matches!(invocation.authority, CallAuthority::Object)
            || invocation.source != ObjectAddress::Fleet
            || invocation.source_face != CallerKind::Registry
        {
            return Err(refused());
        }
        self.delivery().end_peer_sessions(invocation).await
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
