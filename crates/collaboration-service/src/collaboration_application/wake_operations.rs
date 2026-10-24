//! Wakes and their deliveries: durable reminders in automation storage. Timing workers own the
//! later native submission; these operations never touch a native thread.
use super::ResultByteBudget;
use crate::wakeup_subscription::{WakeResumeError, WakeSubscriptionState};
use agent_automation::WakeupId;
use automation_storage::{
    AutomationStore, StorageError, WakeAction, WakeCreate, WakeListPosition, WakeMutation,
};
use collaboration_protocol::{
    AutomationPage, AutomationPageRequest, DeliveryInspection, DeliveryShowRequest,
    LocalMutationEvidence, LocalMutationState, OperationId, SavedMessage, UuidIdentity,
    WaitUnavailable, WakeFailure, WakeFailureReason, WakeFailureStage, WakeMutationRequest,
    WakeMutationResult, WakeNextAction, WakeNotFound, WakeSendRequest, WakeShowRequest,
    WakeSnapshot, WakeState, WakeWaitOutcome, WakeWaitRequest, WakeWaitResult,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

mod failure;
/// Wake and delivery operations over automation storage.
mod remote;
pub use failure::WakeWaitFailure;
pub(crate) use failure::{WakeFailureContext, wake_failure};
use failure::{invalid_field, storage_reason};

pub struct WakeOperations<'service> {
    service_id: &'service UuidIdentity,
    remote: Option<&'service Arc<crate::MailDeliveryRuntime>>,
    store: Option<&'service Arc<Mutex<AutomationStore>>>,
    /// Concurrent first-fire waits share these permits; a handle without them cannot wait.
    wait_permits: Option<&'service Arc<Semaphore>>,
}

/// How often a first-fire wait checks for new automation events.
const WAKE_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How long a first-fire wait runs when the caller names no timeout.
const DEFAULT_WAKE_WAIT_SECONDS: u32 = 60;
/// The longest a single first-fire wait may run.
const MAX_WAKE_WAIT_SECONDS: u32 = 1500;

impl<'service> WakeOperations<'service> {
    pub(crate) fn new(
        service_id: &'service UuidIdentity,
        store: Option<&'service Arc<Mutex<AutomationStore>>>,
    ) -> Self {
        Self {
            service_id,
            remote: None,
            store,
            wait_permits: None,
        }
    }

    /// Lets this handle run first-fire waits, sharing `wait_permits` with every other wait.
    pub(crate) fn with_wait_capacity(mut self, wait_permits: &'service Arc<Semaphore>) -> Self {
        self.wait_permits = Some(wait_permits);
        self
    }

    fn store(
        &self,
        context: &WakeFailureContext,
    ) -> Result<&'service Arc<Mutex<AutomationStore>>, WakeFailure> {
        self.store.ok_or_else(|| {
            wake_failure(
                context.clone(),
                WakeFailureReason::AutomationUnavailable,
                LocalMutationState::None,
            )
        })
    }

    /// Creates a wake, or replays the wake an earlier request with this operation created.
    pub(crate) async fn wake_send_local(
        &self,
        request: WakeSendRequest,
    ) -> Result<WakeSnapshot, WakeFailure> {
        let context = WakeFailureContext::operation(request.operation_id.clone());
        let store = self.store(&context)?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let timing = serde_json::to_value(request.timing).and_then(serde_json::from_value);
        let expiry = serde_json::to_value(request.expiry).and_then(serde_json::from_value);
        let (Ok(timing), Ok(expiry)) = (timing, expiry) else {
            return Err(wake_failure(
                context,
                WakeFailureReason::InvalidField {
                    field: "timing".into(),
                    constraint: "Use a valid UTC instant or bounded duration.".into(),
                },
                LocalMutationState::None,
            ));
        };
        let created = store
            .lock()
            .await
            .create_wakeup(&WakeCreate {
                operation_id: request.operation_id,
                requested_by: request.requested_by,
                replace: request.replace,
                message: request.message,
                timing,
                expiry,
                now_ms,
            })
            .await;
        match created {
            // Creation and replay return the cursor time of the durable creation receipt.
            Ok(record) => {
                let observed = record.definition.created_at_ms;
                crate::wakeup_projection::snapshot(record, self.service_id, observed).map_err(
                    |()| {
                        wake_failure(
                            context,
                            WakeFailureReason::InvalidRecord,
                            LocalMutationState::Committed,
                        )
                    },
                )
            }
            Err(error) => {
                let mutation = if matches!(error, StorageError::Database(_)) {
                    LocalMutationState::Unknown
                } else {
                    LocalMutationState::None
                };
                Err(wake_failure(context, storage_reason(error), mutation))
            }
        }
    }

    /// Shows one wake as of now.
    pub(crate) async fn wake_show_local(
        &self,
        request: WakeShowRequest,
    ) -> Result<WakeSnapshot, WakeFailure> {
        let context = WakeFailureContext::wakeup(request.wakeup_id.clone());
        let store = self.store(&context)?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let read = store
            .lock()
            .await
            .read_wakeup::<SavedMessage>(&request.wakeup_id)
            .await;
        match read {
            Ok(record) => crate::wakeup_projection::snapshot(record, self.service_id, now_ms)
                .map_err(|()| {
                    wake_failure(
                        context,
                        WakeFailureReason::InvalidRecord,
                        LocalMutationState::None,
                    )
                }),
            Err(error) => Err(wake_failure(
                context,
                storage_reason(error),
                LocalMutationState::None,
            )),
        }
    }

    /// Lists wakes newest first; the page shrinks until its response fits `budget`.
    pub async fn wake_list(
        &self,
        request: AutomationPageRequest,
        budget: ResultByteBudget,
    ) -> Result<AutomationPage<WakeSnapshot>, WakeFailure> {
        let context = WakeFailureContext::default();
        let store = self.store(&context)?;
        let fail = |reason| wake_failure(context.clone(), reason, LocalMutationState::None);
        let digest = crate::automation_collection_cursor::filter_digest(&json!({}))
            .map_err(|()| fail(WakeFailureReason::InvalidRecord))?;
        let after = match request.cursor {
            None => None,
            Some(cursor) => {
                let decoded = crate::automation_collection_cursor::decode(
                    &cursor,
                    self.service_id,
                    "wake/list",
                    &digest,
                )
                .map_err(|_| {
                    fail(invalid_field(
                        "cursor",
                        "Cursor belongs to another service/collection or is malformed; start a fresh listing.",
                    ))
                })?;
                let (Ok(upper), Ok(last)) = (
                    decoded.upper_key.1.try_into(),
                    decoded.last_key.1.try_into(),
                ) else {
                    return Err(fail(invalid_field(
                        "cursor",
                        "Cursor wake identities must be UUIDv7.",
                    )));
                };
                Some(WakeListPosition {
                    upper_created_at_ms: decoded.upper_key.0,
                    upper_wakeup_id: upper,
                    created_at_ms: decoded.last_key.0,
                    wakeup_id: last,
                })
            }
        };
        let page = store
            .lock()
            .await
            .wake_listing_keys(after, u32::from(request.limit))
            .await
            .map_err(|_| fail(WakeFailureReason::AutomationUnavailable))?;
        let now = chrono::Utc::now().timestamp_millis();
        let mut snapshots = Vec::new();
        for key in page.records {
            let shown = if key.remote {
                self.wake_show(WakeShowRequest {
                    wakeup_id: key.wakeup_id,
                })
                .await?
            } else {
                self.wake_show_local(WakeShowRequest {
                    wakeup_id: key.wakeup_id,
                })
                .await?
            };
            snapshots.push(shown);
        }
        self.fit_wake_page(snapshots, page.next, &digest, now, budget)
            .map_err(|()| fail(WakeFailureReason::InvalidRecord))
    }

    fn fit_wake_page(
        &self,
        mut records: Vec<WakeSnapshot>,
        page_next: Option<WakeListPosition>,
        digest: &str,
        _now: i64,
        budget: ResultByteBudget,
    ) -> Result<AutomationPage<WakeSnapshot>, ()> {
        let positions: Vec<_> = records
            .iter()
            .map(|record| {
                String::from(record.definition.created_at.clone())
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .map(|at| (at.timestamp_millis(), record.definition.wakeup_id.clone()))
                    .map_err(|_| ())
            })
            .collect::<Result<_, _>>()?;
        let upper = page_next
            .as_ref()
            .map(|position| {
                (
                    position.upper_created_at_ms,
                    position.upper_wakeup_id.clone(),
                )
            })
            .or_else(|| positions.last().cloned());
        let mut next = page_next;
        loop {
            let next_cursor = next
                .as_ref()
                .map(|position| {
                    crate::automation_collection_cursor::encode(
                        &crate::automation_collection_cursor::CollectionCursor {
                            version: 1,
                            service_id: self.service_id.clone(),
                            collection: "wake/list".into(),
                            upper_key: (
                                position.upper_created_at_ms,
                                position.upper_wakeup_id.as_str().into(),
                            ),
                            last_key: (position.created_at_ms, position.wakeup_id.as_str().into()),
                            filter_digest: digest.to_owned(),
                        },
                    )
                })
                .transpose()?;
            let candidate = AutomationPage {
                records: records.clone(),
                next_cursor,
            };
            if budget.admits(&candidate) {
                return Ok(candidate);
            }
            records.pop();
            let index = records.len().checked_sub(1).ok_or(())?;
            let (created_at_ms, wakeup_id) = positions.get(index).ok_or(())?;
            let (upper_created_at_ms, upper_wakeup_id) = upper.as_ref().ok_or(())?;
            next = Some(WakeListPosition {
                upper_created_at_ms: *upper_created_at_ms,
                upper_wakeup_id: upper_wakeup_id.clone(),
                created_at_ms: *created_at_ms,
                wakeup_id: wakeup_id.clone(),
            });
        }
    }

    /// Pauses a wake; deliveries already handed to a native thread are not recalled.
    pub(crate) async fn wake_pause_local(
        &self,
        request: WakeMutationRequest,
    ) -> Result<WakeMutationResult, WakeFailure> {
        self.mutate(request, WakeAction::Pause).await
    }

    /// Resumes a paused wake.
    pub(crate) async fn wake_resume_local(
        &self,
        request: WakeMutationRequest,
    ) -> Result<WakeMutationResult, WakeFailure> {
        self.mutate(request, WakeAction::Resume).await
    }

    /// Cancels a wake; a cancelled wake is never implicitly recreated.
    pub(crate) async fn wake_cancel_local(
        &self,
        request: WakeMutationRequest,
    ) -> Result<WakeMutationResult, WakeFailure> {
        self.mutate(request, WakeAction::Cancel).await
    }

    async fn mutate(
        &self,
        request: WakeMutationRequest,
        action: WakeAction,
    ) -> Result<WakeMutationResult, WakeFailure> {
        let context = WakeFailureContext {
            operation_id: Some(request.operation_id.clone()),
            wakeup_id: Some(request.wakeup_id.clone()),
        };
        let store = self.store(&context)?;
        let now = chrono::Utc::now().timestamp_millis();
        let mutated = store
            .lock()
            .await
            .mutate_wakeup::<SavedMessage>(&WakeMutation {
                operation_id: request.operation_id,
                wakeup_id: request.wakeup_id,
                action,
                now_ms: now,
            })
            .await;
        match mutated {
            Ok(result) => crate::wakeup_projection::project_mutation(result, self.service_id)
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
    ) -> Result<(&'service Arc<Mutex<AutomationStore>>, OwnedSemaphorePermit), WakeWaitFailure>
    {
        let permit = self
            .wait_permits
            .and_then(|permits| Arc::clone(permits).try_acquire_owned().ok());
        match (self.store, permit) {
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
