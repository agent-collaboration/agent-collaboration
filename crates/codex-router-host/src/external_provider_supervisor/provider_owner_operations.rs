//! Compose provider conversation answers from the owner's settled facts.

use super::{ExternalProviderSupervisor, failure};
use crate::{
    provider_operation_settlement::{ProviderOperationCompletion, effective_settings},
    session_owner_door::OwnerDoorError,
};
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ConversationOperationSettlement, OperationId,
    ProviderHistoryAvailability, ProviderOperationEffect, SessionRef,
};
use collaboration_runtime::ProviderSessionRecord;
use provider_session_machines::{
    ProviderSettingKind,
    session_lifecycle_answers::refusal_answer,
    session_lifecycle_machine::{RestoreVia, SessionLifecycleRefusal},
    session_owner::{
        ObservedSetting, RemoteEffect, RestoreDisposition, RestoreFailure, RestorePhase,
        SessionOwnerRefusal, SessionOwnerRejection, SessionOwnerReply, SettingEffectInProgress,
    },
    session_refusal_answers::{
        RefusalStep, SettingWaitObservation, effect_in_progress_answer, session_busy_answer,
        setting_in_progress_answer, settings_hold_answer,
    },
    session_setting_machine::UnresolvedCause,
};

pub(super) async fn restore(
    supervisor: &ExternalProviderSupervisor,
    operation: OperationId,
    record: Box<ProviderSessionRecord>,
    via: RestoreVia,
) -> ProviderOperationCompletion {
    let target = record.target.clone();
    let policy = record.requested_policy.clone();
    let owner = match supervisor.session_owner_for(&target) {
        Ok(owner) => owner,
        Err(error) => {
            return ProviderOperationCompletion::Failure(owner_failure(operation, target, error));
        }
    };
    let result = {
        let Ok(correlation) = uuid::Uuid::parse_str(operation.as_str()) else {
            return ProviderOperationCompletion::Failure(failure(
                ConversationOperationFailureKind::InvalidRequest,
                ConversationOperationFailureStage::Admission,
                ProviderOperationEffect::None,
                "invalid operation identity",
                operation,
                Some(target),
            ));
        };
        owner.restore_explicit(correlation, via, record).await
    };
    let settled = match result {
        Ok(settled) => settled,
        Err(error @ OwnerDoorError::Refused { .. }) => {
            return ProviderOperationCompletion::Failure(failure(
                ConversationOperationFailureKind::Busy,
                ConversationOperationFailureStage::Admission,
                ProviderOperationEffect::None,
                &derived_owner_refusal(&error).unwrap_or_else(|| error.to_string()),
                operation,
                Some(target),
            ));
        }
        Err(error) => {
            return ProviderOperationCompletion::Failure(owner_failure(operation, target, error));
        }
    };
    let SessionOwnerReply::RestoreFinished(answer) = settled.reply else {
        return ProviderOperationCompletion::Failure(failure(
            ConversationOperationFailureKind::ProtocolViolation,
            ConversationOperationFailureStage::Settlement,
            ProviderOperationEffect::Unknown,
            "the session returned a result for another command",
            operation,
            Some(target),
        ));
    };
    if let Some(warning) = answer.replay_warning(via) {
        tracing::info!(?answer.attempt, "{warning}");
    }
    let result = match answer.disposition {
        RestoreDisposition::Restored { .. } => {
            let read_confirmed = |kind| {
                answer
                    .observed
                    .get(&kind)
                    .and_then(|evidence| match &evidence.value {
                        ObservedSetting::Confirmed(value) => Some(value.0.clone()),
                        _ => None,
                    })
            };
            let mut settings = effective_settings(policy);
            settings.mode = read_confirmed(ProviderSettingKind::Mode);
            settings.model = read_confirmed(ProviderSettingKind::Model);
            settings.effort = read_confirmed(ProviderSettingKind::Effort);
            settings.fast = read_confirmed(ProviderSettingKind::Fast).map(|fast| {
                if matches!(fast.as_str(), "off" | "false") {
                    collaboration_protocol::FastState::Off
                } else {
                    collaboration_protocol::FastState::On
                }
            });
            Ok(match via {
                RestoreVia::Load => ConversationOperationSettlement::Loaded {
                    target: target.clone(),
                    effective_settings: settings,
                },
                RestoreVia::Resume => ConversationOperationSettlement::Resumed {
                    target: target.clone(),
                    effective_settings: settings,
                    history: ProviderHistoryAvailability::HistoryUnavailable,
                },
            })
        }
        RestoreDisposition::Retired { failure, .. }
            if let Some(coded) = failure.coded_evidence() =>
        {
            Err(
                super::provider_operation_failure::restore_operation_failure(
                    operation,
                    target.clone(),
                    acp_client_runtime::RestoreFailure::attempted(coded.into()),
                ),
            )
        }
        RestoreDisposition::Retired {
            failure: reason,
            remote,
        } => {
            let kind = match &reason {
                RestoreFailure::Lost { .. } => {
                    ConversationOperationFailureKind::ProviderSessionNotFound
                }
                RestoreFailure::NoAnswer | RestoreFailure::Disconnected => {
                    ConversationOperationFailureKind::OutcomeUnknown
                }
                _ => ConversationOperationFailureKind::Unavailable,
            };
            Err(failure(
                kind,
                if answer.phase == RestorePhase::Preflight {
                    ConversationOperationFailureStage::Admission
                } else {
                    ConversationOperationFailureStage::Dispatch
                },
                match remote {
                    RemoteEffect::None => ProviderOperationEffect::None,
                    RemoteEffect::Applied => ProviderOperationEffect::Applied,
                    RemoteEffect::Unknown => ProviderOperationEffect::Unknown,
                },
                reason.next_step(),
                operation,
                Some(target.clone()),
            ))
        }
    };
    ProviderOperationCompletion::Owned {
        result,
        target,
        saved: settled.saved,
    }
}

pub(super) fn derived_owner_refusal(error: &OwnerDoorError) -> Option<String> {
    let OwnerDoorError::Refused {
        rejection: SessionOwnerRejection::Refused(refusal),
        snapshot: Some(snapshot),
    } = error
    else {
        return None;
    };
    let busy = || {
        session_busy_answer(
            &snapshot.lifecycle,
            snapshot.turn,
            snapshot
                .projection
                .capabilities
                .as_ref()
                .is_some_and(|report| report.steer),
        )
    };
    match refusal {
        SessionOwnerRefusal::EffectInProgress(effect) => {
            Some(effect_in_progress_answer(*effect, snapshot))
        }
        SessionOwnerRefusal::CannotSteer
        | SessionOwnerRefusal::SettingEffectInProgress(SettingEffectInProgress::PromptInProgress) => {
            Some(busy())
        }
        SessionOwnerRefusal::SettingsUnresolved => {
            Some(settings_hold_answer(UnresolvedCause::OutcomeUnknown))
        }
        SessionOwnerRefusal::SettingEffectInProgress(SettingEffectInProgress::SettingInFlight(
            state,
        )) => Some(setting_in_progress_answer(
            state.as_ref(),
            |step| step != RefusalStep::Accept || !snapshot.settings_block_input,
            SettingWaitObservation::Change,
        )),
        SessionOwnerRefusal::SettingEffectInProgress(SettingEffectInProgress::FencedSetting(
            state,
        )) => Some(setting_in_progress_answer(
            state.as_ref(),
            |step| match step {
                RefusalStep::Set => false,
                // AcceptSettings checks the session-wide unknown-outcome hold;
                // unlike Set, it does not issue a write through the fence.
                RefusalStep::Accept => !snapshot.settings_block_input,
                _ => true,
            },
            SettingWaitObservation::EarlierWriteReply,
        )),
        _ => refusal
            .lifecycle_refusal()
            .map(|refusal| refusal_answer(&snapshot.lifecycle, refusal)),
    }
}

pub(super) fn owner_failure(
    operation: OperationId,
    target: SessionRef,
    error: OwnerDoorError,
) -> ConversationOperationFailure {
    let refusal = match &error {
        OwnerDoorError::Refused {
            rejection: SessionOwnerRejection::Refused(refusal),
            ..
        } => Some(refusal),
        _ => None,
    };
    let public = match refusal {
        Some(
            SessionOwnerRefusal::EffectInProgress(_)
            | SessionOwnerRefusal::SettingEffectInProgress(_),
        ) => Some((ConversationOperationFailureKind::Busy, None)),
        Some(SessionOwnerRefusal::QueueFull) => Some((
            ConversationOperationFailureKind::Overloaded,
            Some("queue full"),
        )),
        Some(SessionOwnerRefusal::SkillSelectionChanged) => Some((
            ConversationOperationFailureKind::InvalidRequest,
            Some("invalid or stale skill selection"),
        )),
        Some(SessionOwnerRefusal::CannotSteer) => {
            Some((ConversationOperationFailureKind::CannotSteer, None))
        }
        Some(SessionOwnerRefusal::SettingsUnresolved) => {
            Some((ConversationOperationFailureKind::SettingsUnresolved, None))
        }
        Some(SessionOwnerRefusal::GenerationChanged) => Some((
            ConversationOperationFailureKind::StaleGeneration,
            refusal.and_then(SessionOwnerRefusal::retry_advice),
        )),
        Some(SessionOwnerRefusal::CancelTargetInactive) => Some((
            ConversationOperationFailureKind::NotFound,
            Some("target provider prompt operation is not active on this binding"),
        )),
        _ => None,
    };
    if let Some((kind, message)) = public {
        let message = message
            .map(str::to_owned)
            .or_else(|| derived_owner_refusal(&error))
            .unwrap_or_else(|| error.to_string());
        return failure(
            kind,
            ConversationOperationFailureStage::Admission,
            ProviderOperationEffect::None,
            &message,
            operation,
            Some(target),
        );
    }
    let effect = if matches!(error, OwnerDoorError::OwnerStopped) {
        ProviderOperationEffect::Unknown
    } else {
        ProviderOperationEffect::None
    };
    failure(
        ConversationOperationFailureKind::Unavailable,
        ConversationOperationFailureStage::Admission,
        effect,
        &derived_owner_refusal(&error).unwrap_or_else(|| error.to_string()),
        operation,
        Some(target),
    )
}

pub(super) async fn close(
    supervisor: &ExternalProviderSupervisor,
    operation: OperationId,
    target: SessionRef,
    supports_close: bool,
) -> ProviderOperationCompletion {
    use provider_session_machines::{
        session_lifecycle_machine::{CloseAttempt, SessionLifecycle},
        session_owner::{CloseOutcome, CloseRequest, RunAttempt, SessionOwnerCommand},
    };
    let owner = match supervisor.session_owner_for(&target) {
        Ok(owner) => owner,
        Err(error) => {
            return ProviderOperationCompletion::Failure(owner_failure(operation, target, error));
        }
    };
    let lifecycle = owner.snapshots().borrow().lifecycle;
    let generation = match lifecycle {
        SessionLifecycle::Live { generation } | SessionLifecycle::Loading { generation, .. } => {
            generation
        }
        SessionLifecycle::Closing { .. } => {
            return ProviderOperationCompletion::Failure(failure(
                ConversationOperationFailureKind::Busy,
                ConversationOperationFailureStage::Admission,
                ProviderOperationEffect::None,
                &refusal_answer(&lifecycle, SessionLifecycleRefusal::CloseInProgress),
                operation,
                Some(target),
            ));
        }
        SessionLifecycle::NotLoaded { .. } => {
            return ProviderOperationCompletion::Failure(failure(
                ConversationOperationFailureKind::NotFound,
                ConversationOperationFailureStage::Admission,
                ProviderOperationEffect::None,
                &refusal_answer(&lifecycle, SessionLifecycleRefusal::NothingToClose),
                operation,
                Some(target),
            ));
        }
    };
    let Ok(correlation) = uuid::Uuid::parse_str(operation.as_str()) else {
        return ProviderOperationCompletion::Failure(failure(
            ConversationOperationFailureKind::InvalidRequest,
            ConversationOperationFailureStage::Admission,
            ProviderOperationEffect::None,
            "invalid operation identity",
            operation,
            Some(target),
        ));
    };
    let settled = owner
        .settle(
            correlation,
            SessionOwnerCommand::CloseWithCapability(CloseRequest {
                run: RunAttempt {
                    generation,
                    attempt: CloseAttempt(correlation),
                },
                provider_supports_close: supports_close,
            }),
        )
        .await;
    let settled = match settled {
        Ok(settled) => settled,
        Err(error) => {
            return ProviderOperationCompletion::Failure(owner_failure(operation, target, error));
        }
    };
    let SessionOwnerReply::CloseFinished { outcome, .. } = settled.reply else {
        return ProviderOperationCompletion::Failure(failure(
            ConversationOperationFailureKind::ProtocolViolation,
            ConversationOperationFailureStage::Settlement,
            ProviderOperationEffect::Unknown,
            "the session returned a result for another command",
            operation,
            Some(target),
        ));
    };
    let evidence = crate::session_close_evidence::classify_close(outcome);
    let result = match outcome {
        CloseOutcome::Confirmed | CloseOutcome::RouterOnly => {
            Ok(ConversationOperationSettlement::Closed {
                target: target.clone(),
                router_closed: outcome == CloseOutcome::RouterOnly,
            })
        }
        _ => Err(failure(
            match outcome {
                CloseOutcome::RefusedLive | CloseOutcome::RefusedLoading => {
                    ConversationOperationFailureKind::ProviderRejected
                }
                CloseOutcome::OutcomeUnknown => ConversationOperationFailureKind::OutcomeUnknown,
                _ => ConversationOperationFailureKind::Unavailable,
            },
            ConversationOperationFailureStage::Settlement,
            evidence.effect,
            evidence.summary,
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

pub(super) async fn cancel(
    supervisor: &ExternalProviderSupervisor,
    operation: OperationId,
    target: SessionRef,
    target_operation: OperationId,
) -> ProviderOperationCompletion {
    let owner = match supervisor.session_owner_for(&target) {
        Ok(owner) => owner,
        Err(error) => {
            return ProviderOperationCompletion::Failure(owner_failure(operation, target, error));
        }
    };
    let Ok(target_id) = uuid::Uuid::parse_str(target_operation.as_str()) else {
        return ProviderOperationCompletion::Failure(failure(
            ConversationOperationFailureKind::InvalidRequest,
            ConversationOperationFailureStage::Admission,
            ProviderOperationEffect::None,
            "invalid target operation identity",
            operation,
            Some(target),
        ));
    };
    #[cfg(test)]
    pause_before_cancel_admission(supervisor).await;
    match owner.cancel_operation(target_id).await {
        Ok(Ok(_drain)) => ProviderOperationCompletion::Success {
            settlement: ConversationOperationSettlement::CancelRequested {
                target: target.clone(),
                target_operation_id: target_operation,
            },
            target: Some(target),
            session_record: None,
        },
        Ok(Err(reason)) => ProviderOperationCompletion::Failure(failure(
            ConversationOperationFailureKind::Unavailable,
            ConversationOperationFailureStage::Dispatch,
            ProviderOperationEffect::None,
            reason.human_phrase(),
            operation,
            Some(target),
        )),
        Err(error) => ProviderOperationCompletion::Failure(owner_failure(operation, target, error)),
    }
}

#[cfg(test)]
async fn pause_before_cancel_admission(supervisor: &ExternalProviderSupervisor) {
    let pause = supervisor
        .inner
        .cancel_admission_test_pause
        .lock()
        .expect("cancel admission test pause")
        .take();
    if let Some(pause) = pause {
        let _entered = pause.entered.send(());
        let _released = pause.release.await;
    }
}
