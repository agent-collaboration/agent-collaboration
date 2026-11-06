//! Test-only fixed clock and ID source; absent from production builds.
use runtime_primitives::{Clock, IdSource, SyncTime};
use std::sync::atomic::{AtomicU64, Ordering};
#[derive(Clone, Copy)]
pub(crate) enum ConformanceCapabilities {
    Runtime,
    Fixed { now: SyncTime },
}
impl ConformanceCapabilities {
    pub(crate) fn from_env(env: &worker::Env) -> worker::Result<Self> {
        match env.var("CENTRAL_CONFORMANCE_FIXED_TIME") {
            Ok(value) => {
                let millis = value.to_string().parse::<i64>().map_err(|_| {
                    worker::Error::RustError("invalid fixed conformance time".into())
                })?;
                Ok(Self::Fixed {
                    now: SyncTime::from_unix_millis(millis),
                })
            }
            Err(_) => Ok(Self::Runtime),
        }
    }
}
impl Clock for ConformanceCapabilities {
    fn now(&self) -> SyncTime {
        match self {
            Self::Fixed { now } => *now,
            Self::Runtime => SyncTime::from_unix_millis(chrono::Utc::now().timestamp_millis()),
        }
    }
}
impl IdSource for ConformanceCapabilities {
    fn new_id(&self) -> durable_steps::Uuid {
        match self {
            Self::Runtime => durable_steps::Uuid::now_v7(),
            Self::Fixed { .. } => {
                static IDS: AtomicU64 = AtomicU64::new(0);
                durable_steps::Uuid::from_u128(
                    0x019f0000000070008000000010000000
                        + u128::from(IDS.fetch_add(1, Ordering::Relaxed)),
                )
            }
        }
    }
}
