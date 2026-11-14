//! One physical Fleet or project owner.
use crate::{CoreHost, LegacyObjectHost, ObjectKind, PlatformIo};
use std::{marker::PhantomData, sync::Arc};

pub struct ObjectRuntime<Face: ObjectKind, Host: LegacyObjectHost, Io: PlatformIo> {
    pub(crate) work: ObjectWork<Host, Io>,
    pub(crate) face: PhantomData<fn() -> Face>,
}

pub(crate) enum ObjectWork<Host: LegacyObjectHost, Io: PlatformIo> {
    Fleet {
        host: Arc<Host>,
        io: Arc<Io>,
        engine: Arc<dyn crate::fleet_task_engine::FleetTaskEngine>,
        shared_host: Arc<CoreHost<Io>>,
        replies: Arc<crate::holder_reply_hub::HolderReplyHub>,
        config: fleet_sync_lease::RegistryConfig,
        delivery: crate::DeliveryConfig,
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
}
