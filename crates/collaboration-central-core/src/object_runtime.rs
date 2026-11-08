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
        /// Which notices each connection was already sent (a hint, in memory).
        notice_cursors: Arc<crate::notice_push::NoticeCursors>,
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
        if let ObjectWork::Fleet { notice_cursors, .. } = &self.work {
            match &event {
                // A closed socket keeps its cursor: the library keeps the session,
                // and notices published meanwhile wait in its transport log.
                ObjectEvent::SocketOpened { binding } => {
                    notice_cursors.opened(binding.peer, binding.connection)
                }
                ObjectEvent::SocketClosed { .. }
                | ObjectEvent::Frame { .. }
                | ObjectEvent::Call { .. }
                | ObjectEvent::Alarm => {}
            }
        }
        self.handle_fleet_event(event).await
    }

    async fn finish_event(&self, owed: crate::EventOwed) {
        // Notices the event committed go out now, after all its transactions.
        // A machine whose connection just attached is caught up on its owed
        // requests too; read before the notice push clears its catch-up mark.
        let mut requests = owed.requests;
        if let ObjectWork::Fleet { notice_cursors, .. } = &self.work {
            requests.extend(notice_cursors.catching_up());
        }
        self.send_owed_requests(requests).await;
        self.push_notices(owed.notices).await;
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

    async fn finish_event(&self, _owed: crate::EventOwed) {}

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

#[cfg(feature = "conformance")]
mod conformance_snapshot_read;
