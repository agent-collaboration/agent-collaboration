//! Wake commands follow stored local or central ownership; separate drivers perform delivery.
use crate::wakeup_subscription::{WakeResumeError, WakeSubscriptionState};
use agent_automation::WakeupId;
use automation_storage::{AutomationStore, StorageError};
use collaboration_protocol::{
    DeliveryInspection, DeliveryShowRequest, LocalMutationEvidence, LocalMutationState,
    OperationId, SavedMessage, UuidIdentity, WakeFailure, WakeFailureReason, WakeFailureStage,
    WakeMutationResult, WakeNextAction, WakeShowRequest, WakeSnapshot, WakeState, WakeWaitOutcome,
    WakeWaitRequest, WakeWaitResult,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

mod catalog_mutations;
mod catalog_reads;
mod catalog_wait;
mod failure;
#[cfg(test)]
mod legacy_fixture;
mod legacy_recovery;
mod list_cursor;
/// Wake and delivery operations over automation storage.
pub use crate::wake_operation_failure::WakeWaitFailure;
pub(crate) use failure::{WakeFailureContext, wake_failure};

pub struct WakeOperations {
    catalog: crate::central_catalog::CentralCatalog,
    service_id: UuidIdentity,
    clock: Arc<dyn crate::SubscriptionClock>,
    store: Option<Arc<Mutex<AutomationStore>>>,
    legacy_wake_recovery: Option<Arc<crate::MailDeliveryRuntime>>,
    /// Concurrent first-fire waits share these permits; a handle without them cannot wait.
    wait_permits: Option<Arc<Semaphore>>,
}

/// How often a first-fire wait checks for new automation events.
const WAKE_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How long a first-fire wait runs when the caller names no timeout.
const DEFAULT_WAKE_WAIT_SECONDS: u32 = 60;
/// The longest a single first-fire wait may run.
const MAX_WAKE_WAIT_SECONDS: u32 = 1500;

impl WakeOperations {
    pub fn new(service_id: &UuidIdentity, store: Option<&Arc<Mutex<AutomationStore>>>) -> Self {
        Self {
            catalog: crate::central_catalog::CentralCatalog::new(None),
            service_id: service_id.clone(),
            clock: Arc::new(crate::SystemSubscriptionClock),
            store: store.cloned(),
            legacy_wake_recovery: None,
            wait_permits: None,
        }
    }

    pub fn with_catalog(
        mut self,
        transport: Option<Arc<dyn collaboration_runtime::CentralAutomationTransport>>,
    ) -> Self {
        self.catalog = crate::central_catalog::CentralCatalog::new(transport);
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn crate::SubscriptionClock>) -> Self {
        self.clock = clock;
        self
    }

    /// Read-only old-operation recovery; remove after the 32-day retention following release.
    pub fn with_legacy_wake_recovery(
        mut self,
        mail: Option<&Arc<crate::MailDeliveryRuntime>>,
    ) -> Self {
        self.legacy_wake_recovery = mail.cloned();
        self
    }

    /// Lets this handle run first-fire waits, sharing `wait_permits` with every other wait.
    pub fn with_wait_capacity(mut self, wait_permits: &Arc<Semaphore>) -> Self {
        self.wait_permits = Some(wait_permits.clone());
        self
    }

    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    fn store(
        &self,
        context: &WakeFailureContext,
    ) -> Result<&Arc<Mutex<AutomationStore>>, WakeFailure> {
        self.store.as_ref().ok_or_else(|| {
            wake_failure(
                context.clone(),
                WakeFailureReason::AutomationUnavailable,
                LocalMutationState::None,
            )
        })
    }

    #[expect(clippy::result_large_err, reason = "Preserve local route evidence.")]
    fn project_local_mutation(
        &self,
        mutated: Result<automation_storage::WakeMutationResult<SavedMessage>, StorageError>,
        context: WakeFailureContext,
    ) -> Result<WakeMutationResult, WakeFailure> {
        match mutated {
            Ok(result) => crate::wakeup_projection::project_mutation(result, &self.service_id)
                .map_err(|()| {
                    wake_failure(
                        context,
                        WakeFailureReason::InvalidRecord,
                        LocalMutationState::Committed,
                    )
                }),
            Err(error) => {
                let mutation = if matches!(error, StorageError::Database(_)) {
                    LocalMutationState::Unknown
                } else {
                    LocalMutationState::None
                };
                let reason = match error {
                    StorageError::WakeNotFound => WakeFailureReason::ResourceNotFound,
                    StorageError::OperationConflict => WakeFailureReason::OperationConflict,
                    StorageError::WakeLifecycleConflict { .. } => {
                        WakeFailureReason::LifecycleConflict
                    }
                    StorageError::Database(_) => WakeFailureReason::AutomationUnavailable,
                    _ => WakeFailureReason::InvalidRecord,
                };
                Err(wake_failure(context, reason, mutation))
            }
        }
    }

    /// Shows one wake delivery and its retained native evidence.
    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    pub async fn delivery_show(
        &self,
        request: DeliveryShowRequest,
    ) -> Result<DeliveryInspection, WakeFailure> {
        let context = WakeFailureContext::default();
        let store = self.store(&context)?;
        let read = store.lock().await.read_delivery(&request.delivery_id).await;
        let Ok(record) = read else {
            let mut failure = wake_failure(
                context,
                WakeFailureReason::ResourceNotFound,
                LocalMutationState::None,
            );
            failure.message = format!(
                "Delivery {} was not found; for a push record, run agent-collaboration show <link>.",
                request.delivery_id.as_str()
            );
            return Err(failure);
        };
        crate::delivery_projection::snapshot(record).map_err(|()| {
            wake_failure(
                context,
                WakeFailureReason::InvalidRecord,
                LocalMutationState::None,
            )
        })
    }

    /// Waits up to `timeoutSeconds` for a wake-up's first fire, or for a change that rules one
    /// out. Starts from the current state, or resumes after `after`, and always returns the
    /// cursor a later wait resumes from, so no change is lost between calls.
    ///
    /// The bound covers every store read, so a wait never outlasts it behind a held store: a
    /// start that cannot read the wake-up in time is unavailable (it has no cursor yet), and a
    /// resume or an observation read that cannot finish in time times out at the last cursor
    /// it reached. An observation read is dropped before it advances its position.
    pub(crate) async fn wake_wait_local(
        &self,
        request: WakeWaitRequest,
    ) -> Result<WakeWaitResult, WakeWaitFailure> {
        let wakeup_id = request.wakeup_id.clone();
        let timeout_seconds = request.timeout_seconds.unwrap_or(DEFAULT_WAKE_WAIT_SECONDS);
        if !(1..=MAX_WAKE_WAIT_SECONDS).contains(&timeout_seconds) {
            return Err(WakeWaitFailure::InvalidField {
                field: "timeoutSeconds",
                constraint: "Wait between 1 and 1500 seconds.",
            });
        }
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(u64::from(timeout_seconds));
        let timed_out = |wakeup_id, cursor| WakeWaitResult {
            wakeup_id,
            outcome: WakeWaitOutcome::TimedOut,
            cursor,
        };
        let (store, permit) = self.wait_admission(&wakeup_id)?;
        let mut observation = match request.after {
            None => {
                let started = tokio::time::timeout_at(
                    deadline,
                    crate::wakeup_subscription::start(
                        Arc::clone(store),
                        self.service_id.clone(),
                        WakeShowRequest {
                            wakeup_id: wakeup_id.clone(),
                        },
                        permit,
                    ),
                )
                .await
                .map_err(|_| WakeWaitFailure::unavailable(wakeup_id.clone()))?;
                let (observation, subscription) = started.map_err(|error| match error {
                    StorageError::WakeNotFound => WakeWaitFailure::not_found(wakeup_id.clone()),
                    _ => WakeWaitFailure::unavailable(wakeup_id.clone()),
                })?;
                if let Some(outcome) = settled_outcome(&subscription.snapshot) {
                    return Ok(WakeWaitResult {
                        wakeup_id,
                        outcome,
                        cursor: subscription.after,
                    });
                }
                observation
            }
            Some(cursor) => {
                let resumed = tokio::time::timeout_at(
                    deadline,
                    WakeSubscriptionState::resume(
                        Arc::clone(store),
                        self.service_id.clone(),
                        wakeup_id.clone(),
                        &cursor,
                        permit,
                    ),
                )
                .await;
                // A resume reads the store only after it has accepted the cursor's form.
                let Ok(resumed) = resumed else {
                    return Ok(timed_out(wakeup_id, cursor));
                };
                resumed.map_err(|error| match error {
                    WakeResumeError::InvalidCursor => WakeWaitFailure::InvalidField {
                        field: "after",
                        constraint: "Use a cursor an earlier wait on this service returned, or omit it.",
                    },
                    WakeResumeError::NotFound => WakeWaitFailure::not_found(wakeup_id.clone()),
                    WakeResumeError::Unavailable => {
                        WakeWaitFailure::unavailable(wakeup_id.clone())
                    }
                })?
            }
        };
        loop {
            let read = tokio::time::timeout_at(deadline, observation.next_changes()).await;
            let changes = match read {
                Ok(changes) => {
                    changes.map_err(|_| WakeWaitFailure::unavailable(wakeup_id.clone()))?
                }
                Err(_) => Vec::new(),
            };
            if let Some(changed) = changes.into_iter().next() {
                return Ok(WakeWaitResult {
                    wakeup_id,
                    outcome: changed.change.into(),
                    cursor: changed.cursor,
                });
            }
            if tokio::time::Instant::now() >= deadline {
                let cursor = observation
                    .cursor()
                    .map_err(|_| WakeWaitFailure::unavailable(wakeup_id.clone()))?;
                return Ok(timed_out(wakeup_id, cursor));
            }
            tokio::time::sleep_until(
                (tokio::time::Instant::now() + WAKE_WAIT_POLL_INTERVAL).min(deadline),
            )
            .await;
        }
    }

    fn wait_admission(
        &self,
        wakeup_id: &WakeupId,
    ) -> Result<(&Arc<Mutex<AutomationStore>>, OwnedSemaphorePermit), WakeWaitFailure> {
        let permit = self
            .wait_permits
            .as_ref()
            .and_then(|permits| Arc::clone(permits).try_acquire_owned().ok());
        match (self.store.as_ref(), permit) {
            (Some(store), Some(permit)) => Ok((store, permit)),
            _ => Err(WakeWaitFailure::unavailable(wakeup_id.clone())),
        }
    }
}

/// A snapshot that already answers a first-fire wait: it fired, or can no longer fire.
fn settled_outcome(snapshot: &WakeSnapshot) -> Option<WakeWaitOutcome> {
    if let Some(fire) = snapshot.first_fire.clone() {
        return Some(WakeWaitOutcome::Fired { fire });
    }
    match snapshot.state {
        WakeState::Active => None,
        WakeState::Paused => Some(WakeWaitOutcome::Paused),
        WakeState::Cancelled => Some(WakeWaitOutcome::Cancelled),
        WakeState::Expired => Some(WakeWaitOutcome::Expired),
        WakeState::Finished => Some(WakeWaitOutcome::FinishedWithoutFiring),
    }
}
