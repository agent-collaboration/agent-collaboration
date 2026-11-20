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
    session_lifecycle_machine::RestoreVia,
    session_owner::{
        ObservedSetting, RemoteEffect, RestoreDisposition, RestoreFailure, RestorePhase,
        SessionOwnerReply,
    },
};

pub(super) async fn restore(
    supervisor: &ExternalProviderSupervisor,
    operation: OperationId,
    record: Box<ProviderSessionRecord>,
    via: RestoreVia,
) -> ProviderOperationCompletion {
    let target = record.target.clone();
    let policy = record.requested_policy.clone();
    let result = match supervisor.session_owner_for(&target) {
        Ok(owner) => {
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
        }
        Err(error) => Err(error),
    };
    let settled = match result {
        Ok(settled) => settled,
        Err(error @ OwnerDoorError::Refused(_)) => {
            return ProviderOperationCompletion::Failure(failure(
                ConversationOperationFailureKind::Busy,
                ConversationOperationFailureStage::Admission,
                ProviderOperationEffect::None,
                &error.to_string(),
                operation,
                Some(target),
            ));
        }
        Err(error) => {
            return ProviderOperationCompletion::Failure(owner_unavailable(
                operation, target, error,
            ));
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

pub(super) fn owner_unavailable(
    operation: OperationId,
    target: SessionRef,
    error: OwnerDoorError,
) -> ConversationOperationFailure {
    use provider_session_machines::session_owner::{SessionOwnerRefusal, SessionOwnerRejection};
    let refusal = match &error {
        OwnerDoorError::Refused(SessionOwnerRejection::Refused(refusal)) => Some(refusal),
        _ => None,
    };
    let public = match refusal {
        Some(SessionOwnerRefusal::EffectInProgress) => Some((
            ConversationOperationFailureKind::Busy,
            "the session is busy; send again",
        )),
        Some(SessionOwnerRefusal::QueueFull) => {
            Some((ConversationOperationFailureKind::Overloaded, "queue full"))
        }
        Some(SessionOwnerRefusal::SkillSelectionChanged) => Some((
            ConversationOperationFailureKind::InvalidRequest,
            "invalid or stale skill selection",
        )),
        Some(SessionOwnerRefusal::CannotSteer) => Some((
            ConversationOperationFailureKind::CannotSteer,
            "cannot steer: use --delivery queue, or run conversation cancel before sending again",
        )),
        Some(SessionOwnerRefusal::SettingsUnresolved) => Some((
            ConversationOperationFailureKind::SettingsUnresolved,
            "settings-uncertain: run conversation settings set to resolve the unknown provider setting outcome",
        )),
        Some(SessionOwnerRefusal::GenerationChanged) => Some((
            ConversationOperationFailureKind::StaleGeneration,
            "the session was restored again; send again if still needed",
        )),
        _ => None,
    };
    if let Some((kind, message)) = public {
        return failure(
            kind,
            ConversationOperationFailureStage::Admission,
            ProviderOperationEffect::None,
            message,
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
        &error.to_string(),
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
            return ProviderOperationCompletion::Failure(owner_unavailable(
                operation, target, error,
            ));
        }
    };
    let generation = match owner.snapshots().borrow().lifecycle {
        SessionLifecycle::Live { generation } | SessionLifecycle::Loading { generation, .. } => {
            generation
        }
        SessionLifecycle::Closing { .. } => {
            return ProviderOperationCompletion::Failure(failure(
                ConversationOperationFailureKind::Busy,
                ConversationOperationFailureStage::Admission,
                ProviderOperationEffect::None,
                "the session is already closing",
                operation,
                Some(target),
            ));
        }
        SessionLifecycle::NotLoaded { .. } => {
            return ProviderOperationCompletion::Failure(failure(
                ConversationOperationFailureKind::NotFound,
                ConversationOperationFailureStage::Admission,
                ProviderOperationEffect::None,
                "session is not loaded; nothing to close; load it or resume it",
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
            return ProviderOperationCompletion::Failure(owner_unavailable(
                operation, target, error,
            ));
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
            return ProviderOperationCompletion::Failure(owner_unavailable(
                operation, target, error,
            ));
        }
    };
    let snapshot = owner.snapshots().borrow().clone();
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
    let cancelled = if let Some(queued) = snapshot
        .queue
        .iter()
        .find(|entry| !entry.started && entry.input.operation == target_id)
    {
        owner
            .cancel_queued(queued.input.input.clone())
            .await
            .map(|()| Ok(provider_session_machines::session_owner::CancelDrainReport::Drained))
    } else if let Some(foreground) = snapshot
        .foreground
        .filter(|input| input.operation == target_id)
    {
        owner
            .cancel_turn(foreground.generation, foreground.turn)
            .await
    } else {
        return ProviderOperationCompletion::Failure(failure(
            ConversationOperationFailureKind::NotFound,
            ConversationOperationFailureStage::Admission,
            ProviderOperationEffect::None,
            "target provider prompt operation is not active on this binding",
            operation,
            Some(target),
        ));
    };
    match cancelled {
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
        Err(error) => {
            ProviderOperationCompletion::Failure(owner_unavailable(operation, target, error))
        }
    }
}
