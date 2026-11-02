//! The existing Host clock bounds central observation; only central admits a Firing.
use super::{DEFAULT_WAKE_WAIT_SECONDS, MAX_WAKE_WAIT_SECONDS, WakeOperations, WakeWaitFailure};
use crate::{
    CentralCatalogFailure,
    wake_operation_failure::{WakeOperationFailure, WakeSelectionFailure},
    wake_wait_results::{CentralWakeWaitOutcome, CentralWakeWaitResult, WakeWaitResponse},
};
use automation_storage::StoredAutomationOwner;
use collaboration_protocol::{
    AutomationAnswer, AutomationCall, AutomationKind, WakeShowRequest, WakeState, WakeWaitRequest,
    WakeupId,
};
use std::{sync::Arc, time::Duration};

impl WakeOperations {
    pub async fn wake_wait_until_first_fire(
        &self,
        request: WakeWaitRequest,
    ) -> Result<WakeWaitResponse, WakeOperationFailure> {
        let seconds = request.timeout_seconds.unwrap_or(DEFAULT_WAKE_WAIT_SECONDS);
        if !(1..=MAX_WAKE_WAIT_SECONDS).contains(&seconds) {
            return Err(WakeWaitFailure::InvalidField {
                field: "timeoutSeconds",
                constraint: "Wait between 1 and 1500 seconds.",
            }
            .into());
        }
        let unavailable = || {
            WakeOperationFailure::Selection(WakeSelectionFailure::LocalStoreUnavailable {
                operation_id: None,
                wakeup_id: Some(request.wakeup_id.clone()),
                message: "Local Wake ownership could not be selected; no owner was guessed.".into(),
            })
        };
        let store = self.store.as_ref().ok_or_else(unavailable)?;
        // Selection is a separate bounded phase: no owner or cursor is assumed yet.
        let selection_deadline =
            self.clock.monotonic_now() + Duration::from_secs(u64::from(seconds));
        let selection = async {
            store
                .lock()
                .await
                .wake_command_owner(&request.wakeup_id)
                .await
        };
        let owner = tokio::select! {
            () = self.clock.sleep_until(selection_deadline) => return Err(unavailable()),
            result = selection => result.map_err(|_| unavailable())?,
        };
        if owner != StoredAutomationOwner::Central {
            // The Local wait owns its deadline and the cursor returned at that deadline.
            return self
                .wake_wait_local(request)
                .await
                .map(|value| WakeWaitResponse::Local { value })
                .map_err(Into::into);
        }
        let deadline = self.clock.monotonic_now() + Duration::from_secs(u64::from(seconds));
        let _permit = self
            .wait_permits
            .as_ref()
            .and_then(|permits| Arc::clone(permits).try_acquire_owned().ok())
            .ok_or_else(|| WakeWaitFailure::unavailable(request.wakeup_id.clone()))?;
        // A cursor from a formerly local wait has no meaning after its owner moved.
        // The requested Wake ID is the complete central handle.
        self.wait_for_central_wake(request.wakeup_id, deadline)
            .await
    }

    async fn wait_for_central_wake(
        &self,
        id: WakeupId,
        deadline: tokio::time::Instant,
    ) -> Result<WakeWaitResponse, WakeOperationFailure> {
        let mut backoff = Duration::from_millis(100);
        loop {
            if self.clock.monotonic_now() >= deadline {
                return Ok(WakeWaitResponse::pending(id));
            }
            let answer = tokio::select! {
                () = self.clock.sleep_until(deadline) => return Ok(WakeWaitResponse::pending(id)),
                answer = self.catalog.request(AutomationCall::WakeShow(WakeShowRequest { wakeup_id: id.clone() }), None) => answer,
            };
            let snapshot = match answer {
                Ok(AutomationAnswer::Suspended(value))
                    if value.kind == AutomationKind::Wake
                        && value.automation_id.as_str() == id.as_str() =>
                {
                    return Ok(WakeWaitResponse::suspended(value));
                }
                Ok(AutomationAnswer::Wake(snapshot)) if snapshot.definition.wakeup_id == id => {
                    snapshot
                }
                Ok(AutomationAnswer::Faulted(fault))
                    if fault.kind == AutomationKind::Wake
                        && fault.automation_id.as_str() == id.as_str() =>
                {
                    return Ok(WakeWaitResponse::faulted(fault));
                }
                Err(CentralCatalogFailure::CentralUnreachable { .. }) => {
                    return Ok(WakeWaitResponse::pending(id));
                }
                Err(error) => return Err(error.into()),
                _ => return Err(CentralCatalogFailure::InvalidReply.into()),
            };
            let outcome = if let Some(firing) = snapshot.first_firing {
                if firing.automation_id.as_str() != id.as_str() {
                    return Err(CentralCatalogFailure::InvalidReply.into());
                }
                Some(CentralWakeWaitOutcome::Fired {
                    firing: Box::new(firing),
                })
            } else {
                match snapshot.state {
                    WakeState::Active => None,
                    WakeState::Paused => Some(CentralWakeWaitOutcome::Paused {}),
                    WakeState::Cancelled => Some(CentralWakeWaitOutcome::Cancelled {}),
                    WakeState::Expired => Some(CentralWakeWaitOutcome::Expired {}),
                    WakeState::Finished => Some(CentralWakeWaitOutcome::FinishedWithoutFiring {}),
                }
            };
            if let Some(outcome) = outcome {
                return Ok(WakeWaitResponse::Central {
                    value: CentralWakeWaitResult {
                        wakeup_id: id,
                        outcome,
                    },
                });
            }
            let now = self.clock.monotonic_now();
            if now >= deadline {
                return Ok(WakeWaitResponse::pending(id));
            }
            let due_delay = snapshot.next_due_at.and_then(|due| {
                chrono::DateTime::parse_from_rfc3339(&String::from(due))
                    .ok()
                    .and_then(|due| {
                        (due.with_timezone(&chrono::Utc) - self.clock.now())
                            .to_std()
                            .ok()
                    })
                    .filter(|delay| !delay.is_zero())
            });
            let pause = due_delay.unwrap_or(backoff);
            self.clock.sleep_until((now + pause).min(deadline)).await;
            backoff = (backoff * 2).min(Duration::from_secs(1));
        }
    }
}
