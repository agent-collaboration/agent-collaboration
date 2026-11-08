//! One physical Fleet or project owner.
use crate::{CoreHost, ObjectHost, ObjectKind, PlatformIo};
use std::{marker::PhantomData, sync::Arc};

pub struct ObjectRuntime<Face: ObjectKind, Host: ObjectHost, Io: PlatformIo> {
    pub(crate) work: ObjectWork<Host, Io>,
    pub(crate) face: PhantomData<fn() -> Face>,
}

pub(crate) enum ObjectWork<Host: ObjectHost, Io: PlatformIo> {
    Fleet {
        organization: fleet_identity_protocol::OrganizationId,
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
        fleet_references: Arc<dyn crate::CopyFleetReferencePort>,
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

impl<Face: ObjectKind, Host: ObjectHost, Io: PlatformIo> ObjectRuntime<Face, Host, Io> {
    pub(crate) fn fleet_address(&self) -> Result<crate::FleetAddress, durable_steps::CallFailure> {
        match &self.work {
            ObjectWork::Fleet { organization, .. } => {
                Ok(crate::FleetAddress::new(organization.clone()))
            }
            ObjectWork::Copy { .. } => Err(durable_steps::CallFailure::NotSent {
                reason: durable_steps::NotSentReason::Unreachable { last_seen: None },
            }),
        }
    }
}
