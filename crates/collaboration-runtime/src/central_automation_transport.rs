//! The receiving Router consumes S4 custody calls without owning central admission or timing.
use collaboration_protocol::{AutomationAnswer, AutomationCall};
use std::{future::Future, pin::Pin};

#[derive(Debug, thiserror::Error)]
pub enum CentralAutomationError {
    #[error("central automation transport unavailable")]
    Unavailable,
    #[error("central automation answer was lost; the operation outcome is unknown")]
    OutcomeUnknown,
    #[error("central automation reply does not match the request")]
    InvalidReply,
    #[error("central automation work is invalid")]
    InvalidWork,
    #[error("session-create admission is not attached")]
    SessionCreateUnavailable,
    #[error(transparent)]
    Storage(#[from] automation_storage::StorageError),
}
pub trait CentralAutomationTransport: Send + Sync {
    fn call(
        &self,
        request: AutomationCall,
    ) -> Pin<Box<dyn Future<Output = Result<AutomationAnswer, CentralAutomationError>> + Send + '_>>;
}
pub trait CentralAutomationClock: Send + Sync {
    fn now_ms(&self) -> i64;
    fn wait_until(&self, deadline_ms: i64) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}
#[derive(Clone, Copy, Default)]
pub struct SystemAutomationClock;
impl CentralAutomationClock for SystemAutomationClock {
    fn now_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
    fn wait_until(&self, deadline_ms: i64) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let delay = u64::try_from(deadline_ms.saturating_sub(self.now_ms())).unwrap_or(0);
        Box::pin(tokio::time::sleep(std::time::Duration::from_millis(delay)))
    }
}
