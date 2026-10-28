//! Native wire receipt translation and emitter-owned classification stay separate.
use crate::message_effect_state::NativeFailureDisposition;
use agent_automation::{NativeEffectEvidence, PreparationEffect, SubmissionEffect};
use collaboration_contracts::NOT_LOADED_REASON;
use collaboration_contracts::NativeDeliveryResult;
use collaboration_contracts::NativeObservedOutcome;
use collaboration_contracts::UnsentCause;
use collaboration_protocol::DeliveryClientReceipt;
use collaboration_protocol::{
    AcceptedResumeEffect, CodexGeneration, DeliveryNextAction, DeliveryOutcome, DeliveryRejection,
    DeliveryRejectionReason, NativeSendAcceptance, SessionReachability, SessionRef,
};
use serde_json::{Value, json};
pub(super) fn not_submitted(
    reason: &str,
    retryable: bool,
    cause: UnsentCause,
) -> NativeDeliveryResult {
    NativeDeliveryResult::not_submitted(reason, retryable, cause)
        .with_reachability(SessionReachability::CodexAppServer)
}
pub fn interpret_native_response(
    target: &SessionRef,
    response: crate::native_message_dispatch::NativeMessageOutcome,
    held_idle_submission: bool,
    effects: &mut NativeEffectEvidence<SessionRef, CodexGeneration>,
) -> NativeDeliveryResult {
    let disposition = match &response {
        crate::native_message_dispatch::NativeMessageOutcome::Accepted(_) => None,
        crate::native_message_dispatch::NativeMessageOutcome::Failed(failure) => {
            Some(failure.disposition)
        }
    };
    let (outcome, client) = match response {
        crate::native_message_dispatch::NativeMessageOutcome::Accepted(receipt) => match receipt {
            receipt
                if receipt.target == *target
                    && effects.generation.as_ref() == Some(&receipt.generation)
                    && effects.client_user_message_id.as_deref()
                        == Some(String::from(receipt.client_user_message_id.clone()).as_str()) =>
            {
                effects.resume = match receipt.resume_effect {
                    AcceptedResumeEffect::Accepted => PreparationEffect::Accepted,
                    AcceptedResumeEffect::NotRequested => PreparationEffect::NotRequested,
                };
                effects.submission = SubmissionEffect::Accepted;
                effects.permission_differences = receipt.permission_differences.clone();
                let client = Some(DeliveryClientReceipt::CodexAppServer(receipt.clone()));
                match receipt.acceptance {
                    NativeSendAcceptance::QueueAccepted { submission_id } => {
                        effects.native_submission_id = Some(String::from(submission_id));
                        (DeliveryOutcome::Queued, client)
                    }
                    NativeSendAcceptance::NativeInputAccepted {
                        operation, turn_id, ..
                    } => {
                        effects.native_turn_id = Some(String::from(turn_id));
                        let outcome = if held_idle_submission
                            && matches!(
                                operation,
                                collaboration_protocol::NativeInputOperation::TurnStart
                            ) {
                            DeliveryOutcome::Started
                        } else {
                            DeliveryOutcome::StartedOrSteered
                        };
                        (outcome, client)
                    }
                    NativeSendAcceptance::SteerAccepted {
                        turn_id,
                        submission_id,
                    } => {
                        effects.native_turn_id = Some(String::from(turn_id));
                        effects.native_submission_id = submission_id.map(String::from);
                        (DeliveryOutcome::Steered, client)
                    }
                }
            }
            _ => {
                effects.submission = SubmissionEffect::Unknown;
                (DeliveryOutcome::Unknown, None)
            }
        },
        crate::native_message_dispatch::NativeMessageOutcome::Failed(response) => {
            let data = response.response.pointer("/error/data");
            effects.resume = match data
                .and_then(|value| value.pointer("/effects/resume"))
                .and_then(Value::as_str)
            {
                Some("notRequested") => PreparationEffect::NotRequested,
                Some("accepted") => PreparationEffect::Accepted,
                Some("rejected") => PreparationEffect::Rejected,
                _ => PreparationEffect::Unknown,
            };
            effects.submission = match data
                .and_then(|value| value.pointer("/effects/submission"))
                .and_then(Value::as_str)
            {
                Some("notDispatched") => SubmissionEffect::NotDispatched,
                Some("rejected") => SubmissionEffect::Rejected,
                _ => SubmissionEffect::Unknown,
            };
            let kind = data
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("outcomeUnknown");
            if effects.resume == PreparationEffect::Unknown
                || effects.submission == SubmissionEffect::Unknown
            {
                (DeliveryOutcome::Unknown, None)
            } else if kind == "threadMissingOrUnmaterialized" {
                (
                    DeliveryOutcome::NotSubmitted {
                        retryable: false,
                        reason: "thread not found or never started; if it was created without a first message, it was lost when the Host restarted — create it again".into(),
                    },
                    None,
                )
            } else if kind == NOT_LOADED_REASON {
                (
                    DeliveryOutcome::NotSubmitted {
                        retryable: true,
                        reason: NOT_LOADED_REASON.into(),
                    },
                    None,
                )
            } else if matches!(
                kind,
                "nativeRejected" | "unsupportedCapability" | "localRejection"
            ) {
                let reason = data
                    .and_then(|value| value.get("reason"))
                    .cloned()
                    .and_then(|value| serde_json::from_value::<DeliveryRejectionReason>(value).ok())
                    .unwrap_or(if kind == "unsupportedCapability" {
                        DeliveryRejectionReason::UnsupportedCapability
                    } else {
                        DeliveryRejectionReason::LocalRejection
                    });
                let next_action = data
                    .and_then(|value| value.get("nextAction"))
                    .cloned()
                    .and_then(|value| serde_json::from_value::<DeliveryNextAction>(value).ok())
                    .unwrap_or(if kind == "localRejection" {
                        DeliveryNextAction::InspectTarget
                    } else {
                        DeliveryNextAction::CorrectRequest
                    });
                let native_failure = data.and_then(|data| {
                    serde_json::from_value::<collaboration_protocol::NativeDeliveryFailure>(json!({
                        "operation": data.get("operation")?,
                        "stage": data.get("stage")?,
                        "cause": data.get("cause")?,
                        "generation": effects.generation.as_ref()?,
                        "resume": effects.resume,
                        "submission": effects.submission,
                    }))
                    .ok()
                });
                (
                    DeliveryOutcome::Rejected(DeliveryRejection {
                        reason,
                        next_action,
                        client_code: data
                            .and_then(|value| value.get("clientCode"))
                            .and_then(Value::as_i64),
                        detail: if reason == DeliveryRejectionReason::HeldByAnotherClient {
                            Some(
                                crate::message_effect_state::HELD_BY_ANOTHER_CLIENT_GUIDANCE
                                    .to_owned(),
                            )
                        } else {
                            native_failure
                                .as_ref()
                                .map(|failure| failure.cause.human_phrase().to_owned())
                        },
                        claims: None,
                        native_failure,
                    }),
                    None,
                )
            } else {
                (
                    DeliveryOutcome::NotSubmitted {
                        retryable: matches!(kind, "unavailable" | "overloaded"),
                        reason: kind.into(),
                    },
                    None,
                )
            }
        }
    };
    let result = match outcome {
        DeliveryOutcome::NotSubmitted { reason, retryable } => match disposition {
            Some(NativeFailureDisposition::Unsent(cause)) => {
                NativeDeliveryResult::not_submitted(reason, retryable, cause)
            }
            _ => NativeDeliveryResult::unknown(),
        },
        DeliveryOutcome::Rejected(rejection) => match disposition {
            Some(NativeFailureDisposition::Unsent(cause)) => {
                NativeDeliveryResult::rejected(rejection, cause)
            }
            _ => NativeDeliveryResult::unknown(),
        },
        DeliveryOutcome::Started => {
            NativeDeliveryResult::observed(NativeObservedOutcome::Started, None, client)
        }
        DeliveryOutcome::Steered => {
            NativeDeliveryResult::observed(NativeObservedOutcome::Steered, None, client)
        }
        DeliveryOutcome::StartedOrSteered => {
            NativeDeliveryResult::observed(NativeObservedOutcome::StartedOrSteered, None, client)
        }
        DeliveryOutcome::Queued => {
            NativeDeliveryResult::observed(NativeObservedOutcome::Queued, None, client)
        }
        DeliveryOutcome::PeerMessageWritten => {
            NativeDeliveryResult::observed(NativeObservedOutcome::PeerMessageWritten, None, client)
        }
        DeliveryOutcome::Unknown => {
            NativeDeliveryResult::observed(NativeObservedOutcome::Unknown, None, client)
        }
    };
    result.with_reachability(SessionReachability::CodexAppServer)
}
