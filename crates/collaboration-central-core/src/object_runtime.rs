//! One physical Fleet or project owner.
use crate::{CoreHost, ObjectKind, ObjectPlatform};
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
