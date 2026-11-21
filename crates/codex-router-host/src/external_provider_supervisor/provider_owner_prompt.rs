//! Submit CLI and MCP input to the owner after Host operation reservation.

use super::{
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ConversationOperationSettlement, ExternalProviderSupervisor,
    OperationId, PreparedOperation, ProviderConversationFuture, ProviderOperationCompletion,
    ProviderOperationEffect, ProviderOperationKind, SessionRef, admitted_submission,
    existing_submission, failure, optional_message_text,
    provider_admission_policy::DispatchMark,
    provider_owner_operations::owner_failure,
    provider_prompt_contents::{PromptBusyPolicy, ProviderPromptContentsRequest},
    runtime_failure,
};
use crate::session_store_lane::STORE_WAIT;
use acp_client_runtime::acp_transport::TransportRun;
use collaboration_protocol::{
    ConversationOperationQueueState, MessageDelivery, NonEmptyText, ProviderOperationStage,
    ProviderPromptStopReason,
};
use provider_session_machines::{
    SessionGeneration,
    session_lifecycle_answers::state_answer,
    session_lifecycle_machine::SessionLifecycle,
    session_owner::{
        OwnerAddress, OwnerInput, PromptAdmission, RemoteEffect, RevokeReason, SessionOwnerReply,
    },
    session_refusal_answers::settings_hold_answer,
    session_setting_machine::UnresolvedCause,
    session_turn_machine::{SessionTurn, TurnId},
};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Copy)]
pub(crate) enum SessionQueuePlan {
    OnRun(TransportRun),
    Restore {
        run: TransportRun,
        attempt: provider_session_machines::session_lifecycle_machine::LoadAttempt,
    },
}

impl SessionQueuePlan {
    pub(crate) fn run(self) -> TransportRun {
        match self {
            Self::OnRun(run) | Self::Restore { run, .. } => run,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WarmRun {
    pub(crate) owner: OwnerAddress,
    pub(crate) generation: SessionGeneration,
}

impl ExternalProviderSupervisor {
    pub(super) fn owner_prompt(
        &self,
        request: ProviderPromptContentsRequest,
    ) -> ProviderConversationFuture<'static, super::ConversationOperationSubmission> {
        let operation = request.operation_id.clone();
        let failure_target = Some(request.target.clone());
        self.retain_operation(operation, failure_target, self.submit_owner_prompt(request))
    }

    pub(super) fn submit_owner_prompt(
        &self,
        request: ProviderPromptContentsRequest,
    ) -> ProviderConversationFuture<'static, super::ConversationOperationSubmission> {
        let deadline = self.admission_deadline();
        let backend = self.clone();
        Box::pin(async move {
            let target = request.target.clone();
            let (binding, runtime) = backend.runtime_binding(
                &target.endpoint,
                &request.operation_id,
                Some(target.clone()),
            )?;
            let prepared = backend
                .admit_operation(
                    deadline,
                    request.operation_id.clone(),
                    ProviderOperationKind::ConversationPrompt,
                    binding.clone(),
                    Some(&target),
                    DispatchMark::Deferred,
                )
                .await?;
            match prepared {
                PreparedOperation::Existing(operation) => Ok(existing_submission(operation)),
                PreparedOperation::Admitted { mut snapshot, live } => {
                    let operation = request.operation_id.clone();
                    let input = request.input_id.clone();
                    match backend
                        .start_admitted_prompt(request, &binding, &runtime)
                        .await
                    {
                        Ok(receipt) => {
                            use crate::session_owner_calls::InputAcceptance;
                            match receipt.accepted.await.unwrap_or(InputAcceptance::Unknown) {
                                InputAcceptance::Started
                                | InputAcceptance::ProviderStarted
                                | InputAcceptance::Steered { .. } => {
                                    snapshot.stage = ProviderOperationStage::MayHaveDispatched;
                                    snapshot.effect = ProviderOperationEffect::Unknown;
                                    live.dispatch_marked();
                                }
                                InputAcceptance::Queued => {
                                    snapshot.queue_state =
                                        Some(ConversationOperationQueueState::RouterQueued);
                                    snapshot.input_id = Some(input);
                                }
                                InputAcceptance::Unsent { .. }
                                | InputAcceptance::Unknown
                                | InputAcceptance::QueuePlanChanged => {}
                            }
                            #[cfg(test)]
                            let owner_backend = backend.clone();
                            backend.spawn_operation(operation.clone(), live, async move {
                                let settled = receipt.terminal.await;
                                #[cfg(test)]
                                if settled.as_ref().is_ok_and(|settled| {
                                    matches!(
                                        settled.reply,
                                        SessionOwnerReply::InputFinished {
                                            remote: RemoteEffect::Applied,
                                            ..
                                        } | SessionOwnerReply::SteerStarted { .. }
                                    )
                                }) {
                                    wait_at_steer_fixture(
                                        &owner_backend.inner.steer_reply_test_pause,
                                    )
                                    .await;
                                }
                                match settled {
                                    Ok(settled) => {
                                        retained_prompt(&runtime, operation, target, settled)
                                    }
                                    Err(_) => ProviderOperationCompletion::Failure(owner_failure(
                                        operation,
                                        target,
                                        crate::session_owner_door::OwnerDoorError::OwnerStopped,
                                    )),
                                }
                            });
                            Ok(admitted_submission(snapshot))
                        }
                        Err(failure) => {
                            let clock = backend.inner.clock.as_ref();
                            let deadline = clock.monotonic_now().after(STORE_WAIT);
                            backend.spawn_operation(
                                operation,
                                Arc::clone(&live),
                                std::future::ready(
                                    ProviderOperationCompletion::PromptStartFailed {
                                        failure: failure.clone(),
                                        deadline,
                                    },
                                ),
                            );
                            Err(prompt_failure_answer(clock, deadline, &live, failure).await)
                        }
                    }
                }
            }
        })
    }

    #[expect(
        clippy::result_large_err,
        reason = "Keeps the operation's typed refusal."
    )]
    async fn start_admitted_prompt(
        &self,
        request: ProviderPromptContentsRequest,
        binding: &collaboration_protocol::ProviderBindingIdentity,
        runtime: &crate::ExternalProviderRuntime,
    ) -> Result<crate::session_owner_door::OwnerDeliveryHandle, ConversationOperationFailure> {
        use provider_session_machines::session_owner::SessionOwnerCommand;
        let target = request.target;
        let operation = request.operation_id;
        let provider_id = String::from(target.session_id.clone());
        let content = runtime
            .prepare_owner_prompt(&provider_id, request.input)
            .await
            .map_err(|error| runtime_failure(operation.clone(), Some(target.clone()), error))?;
        let owner = self
            .session_owner_for(&target)
            .map_err(|error| owner_failure(operation.clone(), target.clone(), error))?;
        let snapshot = owner.snapshots().borrow().clone();
        let generation = match snapshot.lifecycle {
            SessionLifecycle::Live { generation } => generation,
            SessionLifecycle::Loading { generation, .. }
                if !matches!(request.busy, PromptBusyPolicy::RefuseBusy) =>
            {
                generation
            }
            _ => {
                return Err(failure(
                    ConversationOperationFailureKind::Unavailable,
                    ConversationOperationFailureStage::Admission,
                    ProviderOperationEffect::None,
                    &state_answer(&snapshot.lifecycle),
                    operation,
                    Some(target),
                ));
            }
        };
        let id = Uuid::parse_str(operation.as_str()).map_err(|_| {
            failure(
                ConversationOperationFailureKind::InvalidRequest,
                ConversationOperationFailureStage::Admission,
                ProviderOperationEffect::None,
                "invalid operation identity",
                operation.clone(),
                Some(target.clone()),
            )
        })?;
        let supports_steer = snapshot
            .projection
            .capabilities
            .as_ref()
            .is_some_and(|report| report.steer);
        let turn = match (request.busy, snapshot.turn) {
            (PromptBusyPolicy::Send(MessageDelivery::Steer), SessionTurn::Running { turn }) => turn,
            _ => TurnId(Uuid::now_v7()),
        };
        let input = OwnerInput {
            generation,
            operation: id,
            input: request.input_id,
            turn,
        };
        let command = match request.busy {
            PromptBusyPolicy::RefuseBusy => SessionOwnerCommand::Prompt {
                input: input.clone(),
                admission: PromptAdmission::Direct,
            },
            PromptBusyPolicy::Send(MessageDelivery::Auto) => SessionOwnerCommand::Prompt {
                input: input.clone(),
                admission: PromptAdmission::Auto {
                    steering_supported: supports_steer,
                },
            },
            PromptBusyPolicy::Send(MessageDelivery::Queue) => SessionOwnerCommand::Prompt {
                input: input.clone(),
                admission: PromptAdmission::Queue,
            },
            PromptBusyPolicy::Send(MessageDelivery::Steer) => SessionOwnerCommand::Steer {
                input: input.clone(),
                supported: supports_steer,
            },
        };
        owner.remember_approval(
            input.input.clone(),
            crate::ExternalProviderApprovalContext {
                owner_scope: None,
                requester: request.requested_by,
                approver: request.approver,
                target: target.clone(),
                operation_id: operation.clone(),
                binding_generation: binding.generation.clone(),
                binding_retirement: runtime.retirement(),
            },
        );
        #[cfg(test)]
        if matches!(
            request.busy,
            PromptBusyPolicy::Send(MessageDelivery::Auto | MessageDelivery::Steer)
        ) && matches!(snapshot.turn, SessionTurn::Running { .. })
        {
            wait_at_steer_fixture(&self.inner.steer_injection_test_pause).await;
        }
        owner
            .start_delivery(input, content, command)
            .await
            .map_err(|error| {
                let refusal = owner_failure(operation.clone(), target.clone(), error);
                if matches!(request.busy, PromptBusyPolicy::Send(MessageDelivery::Steer))
                    && refusal.kind == ConversationOperationFailureKind::CannotSteer
                {
                    explicit_steer_refusal(
                        refusal,
                        ConversationOperationFailureKind::UnsupportedCapability,
                    )
                } else {
                    refusal
                }
            })
    }
}

pub(super) async fn prompt_failure_answer(
    clock: &dyn router_clock::RuntimeClock,
    deadline: router_clock::ClockInstant,
    live: &super::LiveOperation,
    failure: ConversationOperationFailure,
) -> ConversationOperationFailure {
    let _publication = router_clock::timeout_at(clock, deadline, live.wait_for_result()).await;
    failure
}

pub(super) fn explicit_steer_refusal(
    mut refusal: super::ConversationOperationFailure,
    kind: ConversationOperationFailureKind,
) -> super::ConversationOperationFailure {
    refusal.message = NonEmptyText::try_from(format!(
        "this harness can't take a steer mid-turn; {}",
        String::from(refusal.message.clone()),
    ))
    .unwrap_or(refusal.message);
    refusal.kind = kind;
    refusal
}

#[cfg(test)]
async fn wait_at_steer_fixture(pause: &std::sync::Mutex<Option<super::AdmissionTestPause>>) {
    let pause = pause.lock().expect("steer fixture lock").take();
    if let Some(pause) = pause {
        let _entered = pause.entered.send(());
        let _released = pause.release.await;
    }
}

pub(super) fn retained_prompt(
    runtime: &crate::ExternalProviderRuntime,
    operation: OperationId,
    target: SessionRef,
    settled: crate::session_owner_calls::OwnedSettlement,
) -> ProviderOperationCompletion {
    let refusal = runtime.take_permission_refusal(&operation);
    let result = match settled.reply {
        SessionOwnerReply::PromptFailed { failure, .. } => Err(super::prompt_runtime_failure(
            operation,
            Some(target.clone()),
            failure.into(),
        )),
        SessionOwnerReply::PromptCompleted { ref completion, .. }
            if matches!(completion.local_cause.as_ref(),
                Some(session_event_model::LocalCause::Other(cause)) if cause == "compactionUnconfirmed") =>
        {
            Err(runtime_failure(
                operation,
                Some(target.clone()),
                crate::ExternalProviderRuntimeError::CompactionUnconfirmed,
            ))
        }
        SessionOwnerReply::PromptCompleted { ref completion, .. }
            if completion.local_cause == Some(session_event_model::LocalCause::OutputOverflow) =>
        {
            Err(super::prompt_runtime_failure(
                operation,
                Some(target.clone()),
                crate::ExternalProviderRuntimeError::PromptOutputLimitExceeded,
            ))
        }
        SessionOwnerReply::PromptCompleted { .. } if let Some(refusal) = refusal => Err(failure(
            ConversationOperationFailureKind::PermissionRejected,
            ConversationOperationFailureStage::Settlement,
            ProviderOperationEffect::Applied,
            refusal.message(),
            operation,
            Some(target.clone()),
        )),
        SessionOwnerReply::PromptCompleted { completion, .. } => {
            match optional_message_text(completion.response) {
                Ok(response) => Ok(ConversationOperationSettlement::PromptCompleted {
                    target: target.clone(),
                    stop_reason: crate::session_owner_storage::stop_reason(&completion.stop_reason),
                    response,
                }),
                Err(()) => Err(failure(
                    ConversationOperationFailureKind::ProtocolViolation,
                    ConversationOperationFailureStage::Settlement,
                    ProviderOperationEffect::Applied,
                    "provider returned invalid prompt output",
                    operation,
                    Some(target.clone()),
                )),
            }
        }
        SessionOwnerReply::SteerStarted { .. } => {
            Ok(ConversationOperationSettlement::PromptCompleted {
                target: target.clone(),
                stop_reason: ProviderPromptStopReason::Unknown("startedFromSteer".to_owned()),
                response: None,
            })
        }
        SessionOwnerReply::InputFinished {
            remote: RemoteEffect::Applied,
            ..
        } => Ok(ConversationOperationSettlement::Steered {
            target: target.clone(),
            turn_approver: settled.turn_approver,
        }),
        SessionOwnerReply::InputFinished {
            remote: RemoteEffect::None,
            ..
        } => Err(failure(
            ConversationOperationFailureKind::Unavailable,
            ConversationOperationFailureStage::Dispatch,
            ProviderOperationEffect::None,
            "provider input was not submitted",
            operation,
            Some(target.clone()),
        )),
        SessionOwnerReply::PromptCancelUnconfirmed { .. } => Err(failure(
            ConversationOperationFailureKind::OutcomeUnknown,
            ConversationOperationFailureStage::Dispatch,
            ProviderOperationEffect::Unknown,
            &format!(
                "the harness didn't stop the turn, so its outcome is unknown; close the session, then check it and run operation reconcile for {}",
                operation.as_str(),
            ),
            operation,
            Some(target.clone()),
        )),
        SessionOwnerReply::SteerInvalidReply { .. } => Err(failure(
            ConversationOperationFailureKind::ProtocolViolation,
            ConversationOperationFailureStage::Dispatch,
            ProviderOperationEffect::Unknown,
            "provider returned an invalid steering result",
            operation,
            Some(target.clone()),
        )),
        SessionOwnerReply::InputUnsent { reason, .. } => Err(failure(
            match reason {
                provider_session_machines::session_owner::RevokeReason::SkillSelectionChanged => {
                    ConversationOperationFailureKind::InvalidRequest
                }
                provider_session_machines::session_owner::RevokeReason::SettingsUnresolved => {
                    ConversationOperationFailureKind::SettingsUnresolved
                }
                _ => ConversationOperationFailureKind::Unavailable,
            },
            ConversationOperationFailureStage::Dispatch,
            ProviderOperationEffect::None,
            &if reason == RevokeReason::SettingsUnresolved {
                settings_hold_answer(UnresolvedCause::OutcomeUnknown)
            } else {
                reason.display_detail().into_owned()
            },
            operation,
            Some(target.clone()),
        )),
        _ if runtime.frame_limit_exceeded() => Err(super::prompt_runtime_failure(
            operation,
            Some(target.clone()),
            crate::ExternalProviderRuntimeError::FrameLimitExceeded,
        )),
        _ => Err(failure(
            ConversationOperationFailureKind::OutcomeUnknown,
            ConversationOperationFailureStage::Dispatch,
            ProviderOperationEffect::Unknown,
            "provider dispatch outcome is unknown",
            operation,
            Some(target.clone()),
        )),
    };
    ProviderOperationCompletion::Owned {
        result,
        target,
        saved: settled.saved,
    }
}
