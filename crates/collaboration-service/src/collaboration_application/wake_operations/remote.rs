//! Route remote wake creation and controls to the installation that owns the timer.
use super::{
    Arc, DEFAULT_WAKE_WAIT_SECONDS, Duration, LocalMutationState, MAX_WAKE_WAIT_SECONDS,
    OperationId, WakeAction, WakeFailure, WakeFailureContext, WakeFailureReason,
    WakeMutationRequest, WakeMutationResult, WakeOperations, WakeSendRequest, WakeShowRequest,
    WakeSnapshot, WakeWaitFailure, WakeWaitOutcome, WakeWaitRequest, WakeWaitResult, WakeupId,
    storage_reason, wake_failure,
};
use collaboration_mail_protocol::{
    RemoteWakeCreator, RemoteWakeRequest, RemoteWakeResult, WakeBody,
};
use serde::{Serialize, de::DeserializeOwned};

fn convert<T: DeserializeOwned>(value: impl Serialize) -> Result<T, ()> {
    serde_json::from_value(serde_json::to_value(value).map_err(|_| ())?).map_err(|_| ())
}
const REMOTE_CALL_BOUND: Duration = Duration::from_secs(20);

impl WakeOperations {
    pub fn with_remote(mut self, remote: Option<&Arc<crate::MailDeliveryRuntime>>) -> Self {
        if let Some(remote) = remote {
            self.clock = remote.subscription_clock();
        }
        self.remote = remote.cloned();
        self
    }

    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    pub async fn wake_send(&self, request: WakeSendRequest) -> Result<WakeSnapshot, WakeFailure> {
        if request.message.target.endpoint.service_id == self.service_id {
            return self.wake_send_local(request).await;
        }
        let context = WakeFailureContext::operation(request.operation_id.clone());
        let fail = |reason| wake_failure(context.clone(), reason, LocalMutationState::None);
        let original =
            serde_json::to_vec(&request).map_err(|_| fail(WakeFailureReason::InvalidRecord))?;
        let now = self.clock.now();
        // Freeze relative times at the requester. Transit never extends a wake's deadline.
        let timing = match &request.timing {
            collaboration_protocol::TimingRequest::After { seconds } => {
                collaboration_protocol::TimingRequest::At {
                    at: timestamp(now + chrono::Duration::seconds(i64::from(u32::from(*seconds))))
                        .map_err(|_| fail(WakeFailureReason::InvalidRecord))?,
                }
            }
            timing => timing.clone(),
        };
        let expiry = match &request.expiry {
            collaboration_protocol::ExpiryRequest::After { seconds } => {
                collaboration_protocol::ExpiryRequest::At {
                    at: timestamp(now + chrono::Duration::seconds(i64::from(u32::from(*seconds))))
                        .map_err(|_| fail(WakeFailureReason::InvalidRecord))?,
                }
            }
            expiry => expiry.clone(),
        };
        let deadline = match &expiry {
            collaboration_protocol::ExpiryRequest::At { at } => String::from(at.clone())
                .parse()
                .map_err(|_| fail(WakeFailureReason::InvalidRecord))?,
            collaboration_protocol::ExpiryRequest::None => match &timing {
                collaboration_protocol::TimingRequest::At { at } => {
                    String::from(at.clone())
                        .parse::<chrono::DateTime<chrono::Utc>>()
                        .map_err(|_| fail(WakeFailureReason::InvalidRecord))?
                        + chrono::Duration::days(1)
                }
                _ => now + chrono::Duration::days(7),
            },
            collaboration_protocol::ExpiryRequest::After { .. } => {
                return Err(fail(WakeFailureReason::InvalidRecord));
            }
        };
        let body = WakeBody {
            requested_by: convert(&request.requested_by)
                .map_err(|_| fail(WakeFailureReason::InvalidRecord))?,
            request: RemoteWakeRequest::Send {
                operation_id: convert(&request.operation_id)
                    .map_err(|_| fail(WakeFailureReason::InvalidRecord))?,
                replace: request.replace,
                message: convert(&request.message)
                    .map_err(|_| fail(WakeFailureReason::InvalidRecord))?,
                timing: convert(timing).map_err(|_| fail(WakeFailureReason::InvalidRecord))?,
                expiry: convert(expiry).map_err(|_| fail(WakeFailureReason::InvalidRecord))?,
            },
        };
        let operation = request.operation_id.clone();
        let result = self
            .remote_call(
                context.clone(),
                request.message.target,
                operation.clone(),
                original,
                body,
                deadline,
                REMOTE_CALL_BOUND,
            )
            .await?;
        let RemoteWakeResult::Snapshot(snapshot) = result else {
            return Err(fail(WakeFailureReason::InvalidRecord));
        };
        let snapshot: WakeSnapshot =
            convert(*snapshot).map_err(|_| fail(WakeFailureReason::InvalidRecord))?;
        self.store(&context)?
            .lock()
            .await
            .remember_remote_wake(&operation, &snapshot)
            .await
            .map_err(|_| {
                wake_failure(
                    context,
                    WakeFailureReason::InvalidRecord,
                    LocalMutationState::Committed,
                )
            })?;
        Ok(snapshot)
    }

    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    pub async fn wake_show(&self, request: WakeShowRequest) -> Result<WakeSnapshot, WakeFailure> {
        let context = WakeFailureContext::wakeup(request.wakeup_id.clone());
        let Some(target) = self.remote_target(&request.wakeup_id, &context).await? else {
            return self.wake_show_local(request).await;
        };
        let result = self
            .control(
                context.clone(),
                target,
                OperationId::generate(),
                RemoteWakeRequest::Show {
                    wakeup_id: convert(request.wakeup_id).map_err(|_| {
                        wake_failure(
                            context.clone(),
                            WakeFailureReason::InvalidRecord,
                            LocalMutationState::None,
                        )
                    })?,
                },
                REMOTE_CALL_BOUND,
            )
            .await?;
        match result {
            RemoteWakeResult::Snapshot(snapshot) => convert(*snapshot).map_err(|_| {
                wake_failure(
                    context,
                    WakeFailureReason::InvalidRecord,
                    LocalMutationState::None,
                )
            }),
            _ => Err(wake_failure(
                context,
                WakeFailureReason::InvalidRecord,
                LocalMutationState::None,
            )),
        }
    }
    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    pub async fn wake_pause(
        &self,
        request: WakeMutationRequest,
    ) -> Result<WakeMutationResult, WakeFailure> {
        self.remote_mutation(request, WakeAction::Pause).await
    }
    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    pub async fn wake_resume(
        &self,
        request: WakeMutationRequest,
    ) -> Result<WakeMutationResult, WakeFailure> {
        self.remote_mutation(request, WakeAction::Resume).await
    }
    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    pub async fn wake_cancel(
        &self,
        request: WakeMutationRequest,
    ) -> Result<WakeMutationResult, WakeFailure> {
        self.remote_mutation(request, WakeAction::Cancel).await
    }

    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    async fn remote_mutation(
        &self,
        request: WakeMutationRequest,
        action: WakeAction,
    ) -> Result<WakeMutationResult, WakeFailure> {
        let context = WakeFailureContext {
            operation_id: Some(request.operation_id.clone()),
            wakeup_id: Some(request.wakeup_id.clone()),
        };
        let Some(target) = self.remote_target(&request.wakeup_id, &context).await? else {
            return self.mutate(request, action).await;
        };
        let bad = || {
            wake_failure(
                context.clone(),
                WakeFailureReason::InvalidRecord,
                LocalMutationState::None,
            )
        };
        let operation_id = convert(&request.operation_id).map_err(|_| bad())?;
        let wakeup_id = convert(&request.wakeup_id).map_err(|_| bad())?;
        let control = match action {
            WakeAction::Pause => RemoteWakeRequest::Pause {
                operation_id,
                wakeup_id,
            },
            WakeAction::Resume => RemoteWakeRequest::Resume {
                operation_id,
                wakeup_id,
            },
            WakeAction::Cancel => RemoteWakeRequest::Cancel {
                operation_id,
                wakeup_id,
            },
        };
        match self
            .control(
                context.clone(),
                target,
                request.operation_id,
                control,
                REMOTE_CALL_BOUND,
            )
            .await?
        {
            RemoteWakeResult::Mutation(result) => convert(*result).map_err(|_| bad()),
            _ => Err(bad()),
        }
    }

    pub async fn wake_wait_until_first_fire(
        &self,
        request: WakeWaitRequest,
    ) -> Result<WakeWaitResult, WakeWaitFailure> {
        if self.remote.is_none() {
            return self.wake_wait_local(request).await;
        }
        let seconds = request.timeout_seconds.unwrap_or(DEFAULT_WAKE_WAIT_SECONDS);
        if !(1..=MAX_WAKE_WAIT_SECONDS).contains(&seconds) {
            return Err(WakeWaitFailure::InvalidField {
                field: "timeoutSeconds",
                constraint: "Wait between 1 and 1500 seconds.",
            });
        }
        let bound = Duration::from_secs(u64::from(seconds));
        let deadline = tokio::time::Instant::now() + bound;
        let wall_deadline = self.clock.now() + chrono::Duration::seconds(i64::from(seconds));
        let work = async {
            let context = WakeFailureContext::wakeup(request.wakeup_id.clone());
            let target = self
                .remote_target(&request.wakeup_id, &context)
                .await
                .map_err(|_| WakeWaitFailure::unavailable(request.wakeup_id.clone()))?;
            let Some(target) = target else {
                return self.wake_wait_local(request.clone()).await;
            };
            let fail = || WakeWaitFailure::unavailable(request.wakeup_id.clone());
            let control = RemoteWakeRequest::Wait {
                wakeup_id: convert(&request.wakeup_id).map_err(|_| fail())?,
                after: request.after.clone(),
                deadline: convert(wall_deadline).map_err(|_| fail())?,
            };
            match self
                .control(
                    context,
                    target,
                    OperationId::generate(),
                    control,
                    deadline.saturating_duration_since(tokio::time::Instant::now()),
                )
                .await
            {
                Ok(RemoteWakeResult::Wait(answer)) => convert(answer).map_err(|_| fail()),
                Err(error) => match error.reason {
                    WakeFailureReason::InvalidField { .. } => Err(WakeWaitFailure::InvalidField {
                        field: "after",
                        constraint: "Use the owning service's wait cursor, or omit it.",
                    }),
                    WakeFailureReason::ResourceNotFound => {
                        Err(WakeWaitFailure::not_found(request.wakeup_id.clone()))
                    }
                    _ => Err(fail()),
                },
                _ => Err(fail()),
            }
        };
        match tokio::time::timeout_at(deadline, work).await {
            Ok(result) => result,
            Err(_) => match request.after {
                Some(cursor) => Ok(WakeWaitResult {
                    wakeup_id: request.wakeup_id,
                    outcome: WakeWaitOutcome::TimedOut,
                    cursor,
                }),
                None => Err(WakeWaitFailure::unavailable(request.wakeup_id)),
            },
        }
    }
    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    async fn remote_target(
        &self,
        id: &WakeupId,
        context: &WakeFailureContext,
    ) -> Result<Option<collaboration_protocol::SessionRef>, WakeFailure> {
        let Some(store) = self.store.as_ref() else {
            return Ok(None);
        };
        store
            .lock()
            .await
            .remote_wake_target(id)
            .await
            .map_err(|_| {
                wake_failure(
                    context.clone(),
                    WakeFailureReason::InvalidRecord,
                    LocalMutationState::None,
                )
            })
    }
    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    async fn control(
        &self,
        context: WakeFailureContext,
        target: collaboration_protocol::SessionRef,
        operation: OperationId,
        request: RemoteWakeRequest,
        bound: Duration,
    ) -> Result<RemoteWakeResult, WakeFailure> {
        let deadline = match &request {
            RemoteWakeRequest::Wait { deadline, .. } => deadline.as_datetime(),
            _ => self.clock.now() + chrono::Duration::seconds(30),
        };
        let body = WakeBody {
            request,
            requested_by: RemoteWakeCreator::Owner {},
        };
        let canonical = serde_json::to_vec(&(&target, &body)).map_err(|_| {
            wake_failure(
                context.clone(),
                WakeFailureReason::InvalidRecord,
                LocalMutationState::None,
            )
        })?;
        self.remote_call(context, target, operation, canonical, body, deadline, bound)
            .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The frozen request, correlation and caller deadline are independent inputs."
    )]
    #[expect(clippy::result_large_err, reason = "Preserve typed failure payloads.")]
    async fn remote_call(
        &self,
        context: WakeFailureContext,
        target: collaboration_protocol::SessionRef,
        operation_id: OperationId,
        canonical_request: Vec<u8>,
        body: WakeBody,
        deadline: chrono::DateTime<chrono::Utc>,
        bound: Duration,
    ) -> Result<RemoteWakeResult, WakeFailure> {
        let fail = |reason, mutation| wake_failure(context.clone(), reason, mutation);
        let operation = async {
            let prepared = self
                .store(&context)?
                .lock()
                .await
                .prepare_remote_wake(automation_storage::RemoteWakeMail {
                    operation_id,
                    target,
                    canonical_request,
                    body,
                    valid_until_ms: deadline.timestamp_millis(),
                })
                .await
                .map_err(|error| fail(storage_reason(error), LocalMutationState::None))?;
            // Preparation is durable before any possible dispatch. A retry cannot
            // infer non-execution from its deadline or from an unavailable carrier.
            let mutation = match &prepared {
                automation_storage::RemoteWakePreparation::New(_) => LocalMutationState::None,
                automation_storage::RemoteWakePreparation::Replayed {
                    snapshot: Some(snapshot),
                    ..
                } => {
                    return Ok(RemoteWakeResult::Snapshot(convert(snapshot).map_err(
                        |_| {
                            fail(
                                WakeFailureReason::InvalidRecord,
                                LocalMutationState::Committed,
                            )
                        },
                    )?));
                }
                automation_storage::RemoteWakePreparation::Replayed { .. } => uncertainty(&context),
            };
            let runtime = self
                .remote
                .as_ref()
                .ok_or_else(|| fail(WakeFailureReason::RemoteUnavailable, mutation))?;
            let result = runtime.send_wake_mail(prepared).await.map_err(|reason| {
                fail(
                    convert(reason).unwrap_or(WakeFailureReason::InvalidRecord),
                    uncertainty(&context),
                )
            })?;
            match result {
                RemoteWakeResult::Refused { reason, mutation } => Err(fail(
                    convert(reason).unwrap_or(WakeFailureReason::InvalidRecord),
                    convert(mutation).unwrap_or(LocalMutationState::Unknown),
                )),
                answer => Ok(answer),
            }
        };
        let caller_deadline = tokio::time::Instant::now() + bound;
        #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
        tracing::trace!(
            target: "collaboration_service::runtime_test_remote_wake_deadline",
            timeout_ms = bound.as_secs().saturating_mul(1000),
            "remote-wake-caller-deadline-armed"
        );
        tokio::time::timeout_at(caller_deadline, operation)
            .await
            .map_err(|_| fail(WakeFailureReason::RemoteUnavailable, uncertainty(&context)))?
    }
}
fn timestamp(
    value: chrono::DateTime<chrono::Utc>,
) -> Result<collaboration_protocol::ObservationTimestamp, ()> {
    value
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        .try_into()
        .map_err(|_| ())
}

fn uncertainty(context: &WakeFailureContext) -> LocalMutationState {
    if context.operation_id.is_some() {
        LocalMutationState::Unknown
    } else {
        LocalMutationState::None
    }
}
