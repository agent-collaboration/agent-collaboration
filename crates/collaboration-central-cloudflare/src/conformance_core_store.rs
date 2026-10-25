//! Bootstrap-only capabilities over the production Durable Object store.
use crate::CloudflareStorageHost;
use collaboration_central_core::{CoreHost, ObjectAddress, PlatformIo};
use durable_steps::{CallFailure, NotSentReason};
use fleet_sync_protocol::ReceiverId;
use runtime_primitives::{AlarmOwner, Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use std::sync::Arc;

pub(crate) type ReceiptStoreHost = CloudflareStorageHost<CoreHost<ControlIo>>;
pub(crate) struct ControlIo {
    wake: AlarmOwner,
}
impl ControlIo {
    pub(crate) fn new(wake: AlarmOwner) -> Self {
        Self { wake }
    }
}
impl Clock for ControlIo {
    fn now(&self) -> SyncTime {
        // Bootstrap probes drive their real engine explicitly under a fixed future clock.
        SyncTime::from_unix_millis(1_900_000_000_000)
    }
}
impl IdSource for ControlIo {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for ControlIo {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        self.wake.wake_at(at).await
    }
}
impl PlatformIo for ControlIo {
    async fn object_call(
        &self,
        _to: ObjectAddress,
        _body: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        // A bootstrap fixture provides no remote object. Never synthesize a successful reply.
        Err(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        })
    }
}
pub(crate) fn new_host(
    state: Arc<worker::send::SendWrapper<worker::State>>,
) -> worker::Result<Arc<ReceiptStoreHost>> {
    let receiver = ReceiverId::new(durable_steps::Uuid::now_v7())
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    Ok(Arc::new(CloudflareStorageHost::new(state, move |wake| {
        Arc::new(CoreHost::conformance_registry(
            ControlIo::new(wake),
            receiver,
        ))
    })))
}
