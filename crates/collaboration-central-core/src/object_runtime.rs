//! One physical Fleet or project owner, and each face's half of the event pipeline.
//!
//! The KernelFace impls live in this module rather than beside the kernel: the
//! runtime is inside the crate's existing module cycle, and an impl elsewhere
//! would add new edges into it.
use crate::alarm_scheduler::DueSources;
use crate::object_event::{EventReply, ObjectEvent};
use crate::{
    CopyKind, CoreHost, FleetKind, KernelFace, ObjectAlarm as _, ObjectKind, ObjectPlatform,
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use runtime_primitives::{Clock as _, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

pub struct ObjectRuntime<Face: ObjectKind, Host: ObjectPlatform> {
    pub(crate) work: ObjectWork<Host>,
    pub(crate) face: PhantomData<fn() -> Face>,
}

pub(crate) enum ObjectWork<Host: ObjectPlatform> {
    Fleet {
        host: Arc<Host>,
        io: Arc<Host::Io>,
        engine: Arc<dyn crate::fleet_task_engine::FleetTaskEngine>,
        shared_host: Arc<CoreHost<Host::Io>>,
        replies: Arc<crate::holder_reply_hub::HolderReplyHub>,
        config: fleet_project_lease::RegistryConfig,
        delivery: crate::DeliveryConfig,
        holder_answer_timeout: std::time::Duration,
        honored_capabilities: fleet_protocol::FrameCapabilities,
    },
    Copy {
        host: Arc<Host>,
        database: fleet_protocol::DatabaseId,
        io: Arc<Host::Io>,
        receiver: fleet_protocol::ReceiverId,
        changes: Arc<crate::copy_change_notifier::CopyChangeNotifier>,
        copy_ack_wait: std::time::Duration,
        #[cfg(feature = "conformance")]
        wait_probe: Arc<crate::conformance_copy_wait::CopyWaitProbe>,
    },
}

impl<Face: ObjectKind, Host: ObjectPlatform> ObjectRuntime<Face, Host> {
    /// The object's platform, whichever face this runtime serves.
    pub(crate) fn platform(&self) -> &Arc<Host> {
        match &self.work {
            ObjectWork::Fleet { host, .. } | ObjectWork::Copy { host, .. } => host,
        }
    }
}

impl<Host: ObjectPlatform> KernelFace for ObjectRuntime<FleetKind, Host>
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
    async fn stored_alarm(&self) -> Result<Option<SyncTime>, runtime_primitives::WakeFailure> {
        self.platform().alarm().stored().await
    }

    fn now(&self) -> SyncTime {
        match &self.work {
            ObjectWork::Fleet { io, .. } | ObjectWork::Copy { io, .. } => io.now(),
        }
    }

    async fn open(&self) -> Result<(), StoreError> {
        self.open_fleet().await
    }

    async fn handle(&self, event: ObjectEvent) -> EventReply {
        match event {
            ObjectEvent::SocketOpened { binding } => match self.on_socket_opened(binding).await {
                Ok(()) => EventReply::None,
                Err(failure) => {
                    eprintln!("Fleet socket open failed: {failure}");
                    EventReply::SocketFailed
                }
            },
            ObjectEvent::Frame { binding, frame } => match self.on_frame(binding, frame).await {
                Ok(()) => EventReply::None,
                Err(failure) => {
                    eprintln!("Fleet frame failed: {failure}");
                    EventReply::SocketFailed
                }
            },
            ObjectEvent::SocketClosed { binding } => {
                if let Err(failure) = self.on_socket_closed(binding).await {
                    eprintln!("Fleet socket close failed: {failure}");
                }
                EventReply::None
            }
            ObjectEvent::Call { body, .. } => match self.on_call(body).await {
                Ok(answer) => EventReply::CallAnswer(answer),
                Err(failure) => EventReply::CallFailure(failure),
            },
            ObjectEvent::Alarm => EventReply::AlarmDone,
        }
    }

    async fn run_due_sources(&self, now: SyncTime) {
        self.run_fleet_due_sources(now).await;
    }

    async fn run_deadline_sources(&self, now: SyncTime) {
        self.expire_fleet_forwards(now).await;
    }

    async fn drive_long_operations(&self) {
        self.drive_fleet_operations().await;
    }

    async fn recompute(
        &self,
        next_batch_at: Option<SyncTime>,
    ) -> Result<Option<SyncTime>, StoreError> {
        crate::alarm_scheduler::recompute(
            self.platform(),
            DueSources::Fleet,
            self.now(),
            next_batch_at,
        )
        .await
    }
}

impl<Host: ObjectPlatform> KernelFace for ObjectRuntime<CopyKind, Host>
where
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
    for<'value> f64:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    async fn stored_alarm(&self) -> Result<Option<SyncTime>, runtime_primitives::WakeFailure> {
        self.platform().alarm().stored().await
    }

    fn now(&self) -> SyncTime {
        match &self.work {
            ObjectWork::Fleet { io, .. } | ObjectWork::Copy { io, .. } => io.now(),
        }
    }

    async fn open(&self) -> Result<(), StoreError> {
        // A Copy migrates in its call and due-work preludes, after Health is
        // classified, so opening a fresh Copy only for Health creates nothing.
        Ok(())
    }

    async fn handle(&self, event: ObjectEvent) -> EventReply {
        match event {
            ObjectEvent::Call {
                body,
                storage_bytes,
            } => {
                let health = matches!(
                    self.copy_call_class(&body),
                    Ok(crate::CopyCallClass::Health)
                );
                let answer = match (health, storage_bytes) {
                    (true, Some(storage_bytes)) => self.on_health(body, storage_bytes).await,
                    (true, None) => Err(durable_steps::CallFailure::AnswerLost),
                    (false, _) => self.on_call(body).await,
                };
                match answer {
                    Ok(answer) => EventReply::CallAnswer(answer),
                    Err(failure) => EventReply::CallFailure(failure),
                }
            }
            ObjectEvent::Alarm => EventReply::AlarmDone,
            ObjectEvent::SocketOpened { .. }
            | ObjectEvent::Frame { .. }
            | ObjectEvent::SocketClosed { .. } => {
                eprintln!("Copy object refused a socket event");
                EventReply::SocketFailed
            }
        }
    }

    async fn run_due_sources(&self, _now: SyncTime) {
        if let Err(failure) = self.expire_copy_staged_parts().await {
            eprintln!("Copy staged-part expiry failed: {failure}");
        }
    }

    async fn run_deadline_sources(&self, _now: SyncTime) {}

    async fn drive_long_operations(&self) {
        if let Err(failure) = self.drive_copy_followups().await {
            eprintln!("Copy follow-up drive failed: {failure}");
        }
    }

    async fn recompute(
        &self,
        next_batch_at: Option<SyncTime>,
    ) -> Result<Option<SyncTime>, StoreError> {
        let ObjectWork::Copy { database, .. } = &self.work else {
            return Err(StoreError::Aborted {
                reason: "copy runtime has a different face".into(),
            });
        };
        crate::alarm_scheduler::recompute(
            self.platform(),
            DueSources::Copy {
                subject: database.as_uuid(),
            },
            self.now(),
            next_batch_at,
        )
        .await
    }
}
