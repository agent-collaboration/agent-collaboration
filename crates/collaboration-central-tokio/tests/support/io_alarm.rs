//! Test platforms whose one alarm is their own `Io`, so a fixture that records
//! wake requests through its `Io` keeps observing every wake the object makes.
//!
//! Exact alarm writes are recorded as the same wake request: these fixtures
//! observe requests, not the stored alarm. A test of exact alarm behavior uses
//! the production `ObjectAlarmTimer` instead.
use collaboration_central_core::{CoreHost, ObjectAlarm, ObjectIo};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::Host;
use fleet_tokio_objects::TokioObject;
use runtime_primitives::{SyncTime, WakeFailure, WakeScheduler};
use std::sync::Arc;

pub struct IoAlarm<Io>(Arc<Io>);

impl<Io: WakeScheduler + 'static> WakeScheduler for IoAlarm<Io> {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        self.0.wake_at(at).await
    }
}

impl<Io: WakeScheduler + 'static> ObjectAlarm for IoAlarm<Io> {
    async fn set_exact(&self, at: Option<SyncTime>) -> Result<(), WakeFailure> {
        match at {
            Some(at) => self.0.wake_at(at).await,
            None => Ok(()),
        }
    }
    async fn stored(&self) -> Result<Option<SyncTime>, WakeFailure> {
        Ok(None)
    }
}

pub type IoPlatform<Io, SharedHost = CoreHost<Io>> = TokioStorageHost<Io, SharedHost, IoAlarm<Io>>;

/// The object's platform with `io` as both its I/O and its alarm.
pub fn io_platform<Io: ObjectIo, SharedHost: Host>(
    object: Arc<TokioObject<SharedHost>>,
    io: Arc<Io>,
) -> IoPlatform<Io, SharedHost> {
    TokioStorageHost::new(object, io.clone(), Arc::new(IoAlarm(io)))
}
