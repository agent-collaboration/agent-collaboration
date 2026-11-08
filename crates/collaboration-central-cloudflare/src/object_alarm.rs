//! The Durable Object's one alarm: the shared lower-only owner plus exact writes.
//!
//! Units lower the alarm through the shared `AlarmOwner`, whose writes commit
//! with the storage transaction they run in. Exact writes go straight to the
//! platform alarm. They are safe without the owner's guard because the kernel
//! calls them only from its serialized recompute and repair, after every unit of
//! that event has finished, and the input gate keeps other events out of the
//! recompute's storage transaction.

use chrono::{DateTime, Utc};
use collaboration_central_core::ObjectAlarm;
use runtime_primitives::{AlarmOwner, SyncTime, WakeFailure, WakeScheduler};
use std::{future::Future, sync::Arc};
use worker::{
    ScheduledTime, State,
    send::{SendFuture, SendWrapper},
};

#[derive(Clone)]
pub struct CloudflareObjectAlarm {
    owner: AlarmOwner,
    state: Arc<SendWrapper<State>>,
}

impl CloudflareObjectAlarm {
    pub(crate) fn new(owner: AlarmOwner, state: Arc<SendWrapper<State>>) -> Self {
        Self { owner, state }
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn owner(&self) -> &AlarmOwner {
        &self.owner
    }
}

impl WakeScheduler for CloudflareObjectAlarm {
    fn wake_at(&self, at: SyncTime) -> impl Future<Output = Result<(), WakeFailure>> + Send {
        self.owner.wake_at(at)
    }
}

impl ObjectAlarm for CloudflareObjectAlarm {
    fn set_exact(
        &self,
        at: Option<SyncTime>,
    ) -> impl Future<Output = Result<(), WakeFailure>> + Send {
        SendFuture::new(async move {
            let storage = self.state.storage();
            match at {
                Some(at) => {
                    let absolute: DateTime<Utc> = DateTime::from_timestamp_millis(at.unix_millis())
                        .ok_or(WakeFailure::InvalidDeadline { at })?;
                    // An integer would be read as a relative delay; DateTime is absolute.
                    storage
                        .set_alarm(ScheduledTime::from(absolute))
                        .await
                        .map_err(unavailable)
                }
                None => storage.delete_alarm().await.map_err(unavailable),
            }
        })
    }

    fn stored(&self) -> impl Future<Output = Result<Option<SyncTime>, WakeFailure>> + Send {
        SendFuture::new(async move {
            self.state
                .storage()
                .get_alarm()
                .await
                .map(|stored| stored.map(SyncTime::from_unix_millis))
                .map_err(unavailable)
        })
    }
}

fn unavailable(error: worker::Error) -> WakeFailure {
    WakeFailure::Unavailable {
        message: error.to_string(),
    }
}
