//! Submit CLI and MCP input to the owner after Host operation reservation.

use super::{
    ConversationOperationFailureKind, ConversationOperationFailureStage,
    ConversationOperationSettlement, ExternalProviderSupervisor, OperationId, PreparedOperation,
    ProviderConversationFuture, ProviderOperationCompletion, ProviderOperationEffect,
    ProviderOperationKind, SessionRef, admitted_submission, existing_submission, failure,
    optional_message_text,
    provider_operation_admission::DispatchMark,
    provider_operation_failure::PromptStartFailure,
    provider_owner_operations::owner_failure,
    provider_prompt_contents::{PromptBusyPolicy, ProviderPromptContentsRequest},
    runtime_failure,
};
use crate::session_owner_delivery_evidence::{
    OwnerDeliveryEvidence, ProviderDeliveryEvidence, placement_evidence,
};
use crate::session_store_lane::STORE_WAIT;
use acp_client_runtime::acp_transport::TransportRun;
use collaboration_contracts::AttemptEvidenceSink;
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

#[derive(Clone, Copy)]
enum DeliveryAdmission {
    #[cfg(test)]
    Direct,
    Requested(MessageDelivery),
    SteerExisting(TurnId),
    QueueOnRun(SessionQueuePlan),
    Warm(WarmRun),
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
                        Err(PromptStartFailure::OwnerRefused(failure)) => {
                            let clock = backend.inner.clock.as_ref();
                            let deadline = clock.monotonic_now().after(STORE_WAIT);
                            backend.spawn_operation(
                                operation,
                                Arc::clone(&live),
                                std::future::ready(
                                    ProviderOperationCompletion::PromptAdmissionRefused {
                                        failure: failure.clone(),
                                        deadline,
                                    },
                                ),
                            );
                            let _publication =
                                router_clock::timeout_at(clock, deadline, live.wait_for_result())
                                    .await;
                            Err(failure)
                        }
                        Err(PromptStartFailure::Other(failure)) => {
                            backend
                                .inner
                                .finish(
                                    operation,
                                    live,
                                    ProviderOperationCompletion::Failure(failure.clone()),
                                )
                                .await;
                            Err(failure)
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
    ) -> Result<crate::session_owner_door::OwnerDeliveryHandle, PromptStartFailure> {
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
                )
                .into());
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
                let owner_refused = matches!(
                    &error,
                    crate::session_owner_door::OwnerDoorError::Refused { .. }
                );
                let refusal = owner_failure(operation.clone(), target.clone(), error);
                let refusal =
                    if matches!(request.busy, PromptBusyPolicy::Send(MessageDelivery::Steer))
                        && refusal.kind == ConversationOperationFailureKind::CannotSteer
                    {
                        explicit_steer_refusal(
                            refusal,
                            ConversationOperationFailureKind::UnsupportedCapability,
                        )
                    } else {
                        refusal
                    };
                if owner_refused {
                    PromptStartFailure::OwnerRefused(refusal)
                } else {
                    refusal.into()
                }
            })
    }
}

fn explicit_steer_refusal(
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

fn retained_prompt(
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

impl ExternalProviderSupervisor {
    #[expect(
        clippy::result_large_err,
        reason = "Retains a typed conversation failure for delivery."
    )]
    pub(crate) async fn submit_owned_delivery(
        &self,
        request: ProviderPromptContentsRequest,
        mode: MessageDelivery,
    ) -> Result<crate::session_owner_calls::InputAcceptance, super::ConversationOperationFailure>
    {
        self.submit_owner_delivery_request(request, DeliveryAdmission::Requested(mode), None)
            .await
    }

    #[expect(clippy::result_large_err, reason = "Retains the typed steer refusal.")]
    pub(crate) async fn submit_existing_turn_steer(
        &self,
        request: ProviderPromptContentsRequest,
        turn: TurnId,
    ) -> Result<crate::session_owner_calls::InputAcceptance, super::ConversationOperationFailure>
    {
        self.submit_owner_delivery_request(request, DeliveryAdmission::SteerExisting(turn), None)
            .await
    }

    #[expect(
        clippy::result_large_err,
        reason = "Retains a typed conversation failure for delivery."
    )]
    pub(crate) async fn submit_queued_on_run(
        &self,
        request: ProviderPromptContentsRequest,
        run: SessionQueuePlan,
    ) -> Result<crate::session_owner_calls::InputAcceptance, super::ConversationOperationFailure>
    {
        self.submit_owner_delivery_request(request, DeliveryAdmission::QueueOnRun(run), None)
            .await
    }

    #[expect(
        clippy::result_large_err,
        reason = "Retains a typed conversation failure for delivery."
    )]
    #[cfg(test)]
    pub(super) async fn submit_owned_direct_delivery(
        &self,
        request: ProviderPromptContentsRequest,
    ) -> Result<crate::session_owner_calls::InputAcceptance, super::ConversationOperationFailure>
    {
        self.submit_owner_delivery_request(request, DeliveryAdmission::Direct, None)
            .await
    }

    pub(crate) async fn submit_warm_owner_prompt(
        &self,
        request: ProviderPromptContentsRequest,
        run: WarmRun,
    ) -> Result<crate::session_owner_calls::InputAcceptance, Box<super::ConversationOperationFailure>>
    {
        self.submit_owner_delivery_request(request, DeliveryAdmission::Warm(run), None)
            .await
            .map_err(Box::new)
    }

    #[expect(
        clippy::result_large_err,
        reason = "Retains the existing typed delivery refusal."
    )]
    pub(crate) async fn submit_evidenced_delivery(
        &self,
        request: ProviderPromptContentsRequest,
        mode: MessageDelivery,
        queued_run: Option<SessionQueuePlan>,
        sink: &dyn AttemptEvidenceSink,
        effect: ProviderDeliveryEvidence,
    ) -> Result<crate::session_owner_calls::InputAcceptance, super::ConversationOperationFailure>
    {
        let admission = queued_run.map_or(
            DeliveryAdmission::Requested(mode),
            DeliveryAdmission::QueueOnRun,
        );
        self.submit_owner_delivery_request(request, admission, Some((sink, effect)))
            .await
    }

    #[expect(
        clippy::result_large_err,
        reason = "Retains a typed conversation failure for delivery."
    )]
    async fn submit_owner_delivery_request(
        &self,
        request: ProviderPromptContentsRequest,
        admission: DeliveryAdmission,
        evidence: Option<(&dyn AttemptEvidenceSink, ProviderDeliveryEvidence)>,
    ) -> Result<crate::session_owner_calls::InputAcceptance, super::ConversationOperationFailure>
    {
        use crate::session_owner_calls::InputAcceptance;
        use provider_session_machines::session_owner::SessionOwnerCommand;

        let deadline = self.admission_deadline();
        let target = request.target.clone();
        let operation = request.operation_id.clone();
        let (binding, runtime) =
            self.runtime_binding(&target.endpoint, &operation, Some(target.clone()))?;
        let owner = self
            .session_owner_for(&target)
            .map_err(|error| owner_failure(operation.clone(), target.clone(), error))?;
        let snapshot = owner.snapshots().borrow().clone();
        let (generation, cold_restore) = match (admission, snapshot.lifecycle) {
            (_, SessionLifecycle::Live { generation })
            | (DeliveryAdmission::QueueOnRun(_), SessionLifecycle::Loading { generation, .. }) => {
                (generation, None)
            }
            (
                DeliveryAdmission::QueueOnRun(SessionQueuePlan::Restore { run, attempt }),
                SessionLifecycle::NotLoaded { last },
            ) if last.number().checked_add(1) == Some(run.generation.number()) => {
                (run.generation, Some(attempt))
            }
            (
                DeliveryAdmission::QueueOnRun(expected),
                SessionLifecycle::Closing { generation, .. },
            ) if expected.run().incarnation == snapshot.owner.incarnation
                && expected.run().generation == generation =>
            {
                return Ok(InputAcceptance::Unsent {
                    reason: Some(provider_session_machines::session_owner::RevokeReason::NotLoaded),
                });
            }
            (DeliveryAdmission::QueueOnRun(_), _) => return Ok(InputAcceptance::QueuePlanChanged),
            _ => {
                return Ok(InputAcceptance::Unsent {
                    reason: Some(provider_session_machines::session_owner::RevokeReason::NotLoaded),
                });
            }
        };
        if let DeliveryAdmission::QueueOnRun(expected) = admission
            && (expected.run().incarnation != snapshot.owner.incarnation
                || expected.run().generation != generation)
        {
            return Ok(InputAcceptance::QueuePlanChanged);
        }
        if let DeliveryAdmission::Warm(run) = admission
            && (snapshot.owner != run.owner || generation != run.generation)
        {
            return Ok(InputAcceptance::Unsent { reason: None });
        }
        let preview = match &request.input {
            acp_client_runtime::ProviderTurnInput::Prompt(prompt) => {
                crate::provider_queue_operation_registry::queued_content_preview(&prompt.content)
            }
            acp_client_runtime::ProviderTurnInput::Compact => String::new(),
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
        let provider_id = String::from(target.session_id.clone());
        let content = self
            .within_admission(
                deadline,
                &operation,
                Some(&target),
                runtime.prepare_owner_prompt(&provider_id, request.input),
            )
            .await?
            .map_err(|error| runtime_failure(operation.clone(), Some(target.clone()), error))?;
        let can_queue = matches!(
            admission,
            DeliveryAdmission::Requested(MessageDelivery::Auto | MessageDelivery::Queue)
                | DeliveryAdmission::QueueOnRun(_)
        );
        let live = if can_queue {
            let Some(live) = self
                .reserve_queued_delivery(deadline, &operation, &target, &binding)
                .await?
            else {
                return Ok(InputAcceptance::Unknown);
            };
            live
        } else {
            let PreparedOperation::Admitted { live, .. } = self
                .admit_operation(
                    deadline,
                    operation.clone(),
                    ProviderOperationKind::ConversationPrompt,
                    binding.clone(),
                    Some(&target),
                    DispatchMark::Deferred,
                )
                .await?
            else {
                return Ok(InputAcceptance::Unknown);
            };
            live
        };
        let steering_supported = snapshot
            .projection
            .capabilities
            .as_ref()
            .is_some_and(|report| report.steer);
        let turn = match (admission, snapshot.turn) {
            (DeliveryAdmission::SteerExisting(turn), _) => turn,
            (
                DeliveryAdmission::Requested(MessageDelivery::Steer),
                SessionTurn::Running { turn },
            ) => turn,
            _ => TurnId(Uuid::now_v7()),
        };
        let input_id = request.input_id.clone();
        let input = OwnerInput {
            generation,
            operation: id,
            input: request.input_id,
            turn,
        };
        let command = match admission {
            DeliveryAdmission::Requested(MessageDelivery::Auto) => SessionOwnerCommand::Prompt {
                input: input.clone(),
                admission: PromptAdmission::DeliveryAuto { steering_supported },
            },
            DeliveryAdmission::Requested(MessageDelivery::Queue)
            | DeliveryAdmission::QueueOnRun(_) => SessionOwnerCommand::Prompt {
                input: input.clone(),
                admission: PromptAdmission::Queue,
            },
            DeliveryAdmission::Requested(MessageDelivery::Steer) => SessionOwnerCommand::Steer {
                input: input.clone(),
                supported: steering_supported,
            },
            DeliveryAdmission::SteerExisting(_) => SessionOwnerCommand::SteerExisting {
                input: input.clone(),
                supported: steering_supported,
            },
            #[cfg(test)]
            DeliveryAdmission::Direct => SessionOwnerCommand::Prompt {
                input: input.clone(),
                admission: PromptAdmission::Direct,
            },
            DeliveryAdmission::Warm(_) => SessionOwnerCommand::WarmPrompt(input.clone()),
        };
        let (writer, intake) = if let Some((sink, effect)) = &evidence {
            let (offer, intake) = tokio::sync::oneshot::channel();
            let writer = OwnerDeliveryEvidence::new(
                offer,
                sink.retain_queued_dispatch(),
                effect.clone(),
                runtime.owner_clock(),
            );
            (Some(writer), Some(intake))
        } else {
            (None, None)
        };
        let receipt = if can_queue
            && owner
                .defer_queued_admission(collaboration_runtime::ProviderOperationAdmission {
                    operation_id: operation.clone(),
                    operation_kind: ProviderOperationKind::ConversationPrompt,
                    binding:
                        collaboration_protocol::ConversationBindingIdentity::ExternalProvider {
                            binding: binding.clone(),
                        },
                    admitted_at_ms: self.inner.now_ms(),
                })
                .is_err()
        {
            Err(crate::session_owner_door::OwnerDoorError::Unavailable)
        } else {
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
            owner
                .start_delivery_with_evidence(input, content, command, writer, cold_restore)
                .await
        };
        match receipt {
            Ok(receipt) => {
                let backend = self.clone();
                let (report_acceptance, received) = tokio::sync::oneshot::channel();
                self.spawn_operation(operation.clone(), live, async move {
                    let acceptance = receipt.accepted.await.unwrap_or(InputAcceptance::Unknown);
                    if acceptance == InputAcceptance::Queued {
                        backend.queued_operation_registry().record_queued_with_preview(
                            operation.clone(),
                            target.clone(),
                            binding,
                            input_id,
                            preview,
                        );
                    }
                    let _caller_gone = report_acceptance.send(acceptance);
                    match receipt.terminal.await {
                        Ok(settled) => {
                            if let SessionOwnerReply::InputUnsent { reason, .. } = &settled.reply {
                                let detail = match reason.queue_not_sent_reason() {
                                    Some(provider_session_machines::queued_entry_machine::NotSentReason::CancelledFromQueue) => "queueCancelled".into(),
                                    Some(reason) => reason.human_phrase().into(),
                                    None => reason.display_detail(),
                                };
                                backend
                                    .queued_operation_registry()
                                    .mark_not_submitted(&operation, detail.as_ref());
                            } else {
                                backend.queued_operation_registry().clear(&operation);
                            }
                            retained_prompt(&runtime, operation, target, settled)
                        }
                        Err(_) => ProviderOperationCompletion::Failure(failure(
                            ConversationOperationFailureKind::OutcomeUnknown,
                            ConversationOperationFailureStage::Dispatch,
                            ProviderOperationEffect::Unknown,
                            "provider dispatch outcome is unknown",
                            operation,
                            Some(target),
                        )),
                    }
                });
                if let (Some((sink, effect)), Some(intake)) = (evidence, intake)
                    && let Ok(offer) = intake.await
                {
                    use provider_session_machines::session_owner::EffectOutcome;
                    let mut recorded = offer.recorded;
                    let outcome = tokio::select! {
                        biased;
                        () = recorded.closed() => EffectOutcome::Unknown,
                        result = sink.record(placement_evidence(effect, offer.placement.kind)) => {
                            if result.is_ok() { EffectOutcome::Applied } else { EffectOutcome::Failed }
                        }
                    };
                    let _owner_stopped = recorded.send(outcome);
                }
                Ok(received.await.unwrap_or(InputAcceptance::Unknown))
            }
            Err(error) => {
                if can_queue {
                    owner.release_queued_admission(&operation);
                }
                use provider_session_machines::session_owner::{
                    SessionOwnerRefusal, SessionOwnerRejection,
                };
                if matches!(admission, DeliveryAdmission::QueueOnRun(_))
                    && matches!(
                        error,
                        crate::session_owner_door::OwnerDoorError::Refused {
                            rejection: SessionOwnerRejection::Refused(
                                SessionOwnerRefusal::GenerationChanged
                            ),
                            ..
                        }
                    )
                {
                    self.drop_reservation(&operation, &live, false);
                    return Ok(InputAcceptance::QueuePlanChanged);
                }
                let refusal = owner_failure(operation.clone(), target.clone(), error);
                let refusal = if refusal.kind == ConversationOperationFailureKind::CannotSteer
                    && matches!(
                        admission,
                        DeliveryAdmission::SteerExisting(_)
                            | DeliveryAdmission::Requested(MessageDelivery::Steer)
                    ) {
                    explicit_steer_refusal(refusal, ConversationOperationFailureKind::CannotSteer)
                } else {
                    refusal
                };
                if refusal.effect == ProviderOperationEffect::None {
                    self.queued_operation_registry()
                        .mark_not_submitted(&operation, String::from(refusal.message.clone()));
                }
                let final_failure = refusal.clone();
                self.spawn_operation(operation, live, async move {
                    if can_queue {
                        ProviderOperationCompletion::Owned {
                            result: Err(final_failure),
                            target,
                            saved: true,
                        }
                    } else {
                        ProviderOperationCompletion::Failure(final_failure)
                    }
                });
                Err(refusal)
            }
        }
    }
}
