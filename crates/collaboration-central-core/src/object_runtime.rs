//! One object's lifecycle and its registered shared workflows.

use crate::{CoreHost, ObjectHost, ObjectKind, PlatformIo, RegistryKind};
use durable_steps::{
    AtomicStore, DriveReport, Engine, StoreError,
    storage::{JournalStorage, TransactionScope},
};
use fleet_sync_lease::RegistryConfig;
use runtime_primitives::WakeScheduler;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

pub struct ObjectRuntime<Face: ObjectKind, Host: ObjectHost, Io: PlatformIo> {
    pub(crate) work: ObjectWork<Host, Io>,
    pub(crate) face: PhantomData<fn() -> Face>,
}

pub(crate) enum ObjectWork<Host: ObjectHost, Io: PlatformIo> {
    Fleet {
        host: Arc<Host>,
        io: Arc<Io>,
        engine: Engine<Host::Store, CoreHost<Io>>,
    },
    Registry {
        engine: Engine<Host::Store, CoreHost<Io>>,
        host: Arc<Host>,
        shared_host: Arc<CoreHost<Io>>,
        config: RegistryConfig,
        delivery: crate::DeliveryConfig,
    },
    Session {
        host: Arc<Host>,
        shared_host: Arc<CoreHost<Io>>,
        replies: Arc<crate::holder_reply_hub::HolderReplyHub>,
        installation: message_board::ServiceId,
        holder_answer_timeout: std::time::Duration,
    },
    Copy {
        host: Arc<Host>,
        database: fleet_sync_protocol::DatabaseId,
        io: Arc<Io>,
        receiver: fleet_sync_protocol::ReceiverId,
        changes: Arc<crate::copy_change_notifier::CopyChangeNotifier>,
        copy_ack_wait: std::time::Duration,
        #[cfg(feature = "conformance")]
        wait_probe: Arc<crate::conformance_copy_wait::CopyWaitProbe>,
    },
    Mailbox {
        engine: Engine<Host::Store, CoreHost<Io>>,
        host: Arc<Host>,
        shared_host: Arc<CoreHost<Io>>,
        installation: message_board::ServiceId,
        limits: fleet_sync_delivery::DeliveryLimits,
        #[cfg(feature = "conformance")]
        conformance_limits: std::sync::OnceLock<fleet_sync_delivery::DeliveryLimits>,
        health: Arc<crate::mailbox_step::MailboxHealth>,
    },
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<RegistryKind, Host, Io>
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
    pub fn registry(
        host: Arc<Host>,
        io: Io,
        config: RegistryConfig,
        delivery: crate::DeliveryConfig,
    ) -> Result<Self, StoreError> {
        let shared_host = Arc::new(CoreHost::registry(
            io,
            config.authoritative_receiver,
            crate::registry_peer_lookup::bind(std::sync::Arc::downgrade(&host)),
            Some(crate::registry_lease_store::bind(
                std::sync::Arc::downgrade(&host),
            )),
        ));
        let engine = crate::shared_task_registry::registry_engine::<_, Host::Database, _>(
            Arc::clone(host.store()),
            shared_host.clone(),
            config,
        )?;
        Ok(Self {
            work: ObjectWork::Registry {
                engine,
                host,
                shared_host,
                config,
                delivery,
            },
            face: PhantomData,
        })
    }
    pub async fn on_alarm(&self) -> Result<DriveReport, StoreError> {
        match &self.work {
            ObjectWork::Registry {
                engine,
                shared_host,
                host,
                ..
            } => {
                let driven: Result<DriveReport, StoreError> = async {
                    let report = engine.drive_due().await?;
                    shared_host.repair_lease_publications().await?;
                    Ok(report)
                }
                .await;
                // An early due-read failure precedes the engine's own rearm.
                // Re-read its existing debt even after a failed drive; the
                // journal still owns IDs, deadlines and retry decisions.
                let rearmed = async {
                    let earliest = host
                        .store()
                        .transaction(|scope| Box::pin(async move { scope.earliest().await }))
                        .await?;
                    if let Some(at) = earliest {
                        host.wake().wake_at(at).await.map_err(StoreError::backend)?;
                    }
                    Ok::<_, StoreError>(())
                }
                .await;
                let report = driven?;
                rearmed?;
                Ok(report)
            }
            ObjectWork::Fleet { .. }
            | ObjectWork::Session { .. }
            | ObjectWork::Copy { .. }
            | ObjectWork::Mailbox { .. } => Err(StoreError::Aborted {
                reason: "registry runtime has a different face".into(),
            }),
        }
    }
    pub async fn initialize(&self) -> Result<usize, StoreError> {
        let ObjectWork::Registry {
            host, shared_host, ..
        } = &self.work
        else {
            return Err(StoreError::Aborted {
                reason: "registry runtime has a different face".into(),
            });
        };
        let applied = crate::ObjectBootstrap::new(
            host.clone(),
            runtime_primitives::Clock::now(shared_host.as_ref()),
        )
        .initialize(crate::EngineSchema::Registry)
        .await?;
        shared_host.initialize_lease_publications().await?;
        Ok(applied)
    }
}
