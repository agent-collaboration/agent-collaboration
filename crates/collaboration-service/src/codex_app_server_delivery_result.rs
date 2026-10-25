//! Native wire receipt translation and emitter-owned classification stay separate.
use crate::message_effect_state::NativeFailureDisposition;
use crate::{
    DeliveryClientReceipt, NOT_LOADED_REASON, NativeDeliveryResult, NativeObservedOutcome,
    UnsentCause,
};
use agent_automation::{NativeEffectEvidence, PreparationEffect, SubmissionEffect};
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
pub(super) fn interpret_native_response(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_effect_state::{MessageEffects, NativeFailureKind};
    use agent_automation::CessationEvidence;
    use serde_json::json;

    fn target() -> SessionRef {
        serde_json::from_value(json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native-result-recipient"})).unwrap()
    }
    fn effects() -> NativeEffectEvidence<SessionRef, CodexGeneration> {
        NativeEffectEvidence {
            permission_differences: Vec::new(),
            target: Some(target()),
            generation: None,
            client_user_message_id: None,
            native_turn_id: None,
            native_submission_id: None,
            allocation: PreparationEffect::NotRequested,
            resume: PreparationEffect::NotRequested,
            submission: SubmissionEffect::NotDispatched,
            cessation: CessationEvidence::NotApplicable,
        }
    }
    #[test]
    fn codex_emitter_cause_survives_a_changed_receipt_reason() {
        let emitter = MessageEffects::new(json!(1));
        for explanation in ["staleGeneration", "renamed human explanation"] {
            let mut failure = emitter.failure(NativeFailureKind::StaleGeneration, "inspect");
            failure.response["error"]["data"]["kind"] = json!(explanation);
            let delivery = interpret_native_response(
                &target(),
                crate::native_message_dispatch::NativeMessageOutcome::Failed(failure),
                false,
                &mut effects(),
            );
            assert_eq!(
                delivery.unsent_cause(),
                Some(UnsentCause::Refused(crate::NativeRefusalCode::Rejection(
                    DeliveryRejectionReason::StaleGeneration
                )))
            );
            assert!(
                matches!(&delivery.receipt().outcome,DeliveryOutcome::NotSubmitted {reason,retryable:false} if reason==explanation)
            );
            let receipt = serde_json::to_value(delivery.receipt()).unwrap();
            let canonical =
                crate::mail_push_outcome::local_message_outcome(delivery, true).unwrap();
            assert_eq!(
                serde_json::to_value(canonical.outcome).unwrap(),
                json!({
                    "kind":"refused","value":{"reason":{"kind":"recipientRefused","value":{"code":"staleGeneration"}}}
                })
            );
            assert_eq!(
                serde_json::to_value(canonical.observation.receipt).unwrap(),
                receipt
            );
            assert!(canonical.observation.scheduled_retry_at.is_none());
            assert!(canonical.observation.route_evidence.is_none());
        }
    }
    #[test]
    fn unknown_native_effect_cannot_become_holdable_from_an_unavailable_reason() {
        let mut emitter = MessageEffects::new(json!(1));
        emitter.submission = "unknown";
        let failure = emitter.failure(NativeFailureKind::Unavailable, "start");
        let delivery = interpret_native_response(
            &target(),
            crate::native_message_dispatch::NativeMessageOutcome::Failed(failure),
            false,
            &mut effects(),
        );
        assert_eq!(delivery.unsent_cause(), None);
        assert_eq!(delivery.receipt().outcome, DeliveryOutcome::Unknown);
    }

    #[test]
    fn native_processing_and_policy_refusal_publish_local_rejections_with_distinct_causes() {
        let emitter = MessageEffects::new(json!(1));
        let expected = collaboration_protocol::DeliveryReceipt {
            outcome: DeliveryOutcome::Rejected(DeliveryRejection {
                native_failure: None,
                reason: DeliveryRejectionReason::LocalRejection,
                next_action: DeliveryNextAction::InspectTarget,
                client_code: None,
                detail: None,
                claims: None,
            }),
            reachability: Some(SessionReachability::CodexAppServer),
            client: None,
        };
        let settings = UnsentCause::Refused(crate::NativeRefusalCode::Rejection(
            DeliveryRejectionReason::SettingsUnresolved,
        ));
        for (failure, cause) in [
            (
                emitter.failure(
                    NativeFailureKind::LocalRejection(UnsentCause::ProcessingFailure),
                    "inspect",
                ),
                UnsentCause::ProcessingFailure,
            ),
            (
                emitter.recorded_policy_failure(
                    &codex_acp_adapter::SessionSetupError::ConfigurationMismatch,
                    "resume",
                    collaboration_protocol::NativeDeliveryFailureCause::RecordedResumePreparationFailed,
                ),
                settings,
            ),
        ] {
            let result = interpret_native_response(
                &target(),
                crate::native_message_dispatch::NativeMessageOutcome::Failed(failure),
                false,
                &mut effects(),
            );
            assert_eq!(result.unsent_cause(), Some(cause));
            assert_eq!(
                serde_json::to_value(result.into_receipt()).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
        }
    }

    #[test]
    fn recorded_setup_emitters_map_to_terminal_codes_without_widening_direct_held() {
        use codex_acp_adapter::SessionSetupError as Error;
        // Arrange: these errors all preserve the same pre-existing nativeRejected wire receipt.
        let emitter = MessageEffects::new(json!(1));
        let expected_receipt = collaboration_protocol::DeliveryReceipt {
            outcome: DeliveryOutcome::Rejected(DeliveryRejection {
                native_failure: None,
                reason: DeliveryRejectionReason::LocalRejection,
                next_action: DeliveryNextAction::InspectTarget,
                client_code: None,
                detail: None,
                claims: None,
            }),
            reachability: Some(SessionReachability::CodexAppServer),
            client: None,
        };
        for (error, expected_cause, expected_canonical) in [
            (
                Error::Unavailable,
                UnsentCause::Refused(crate::NativeRefusalCode::NativeUnavailable),
                json!({"kind":"refused","value":{"reason":{"kind":"recipientRefused","value":{"code":"nativeUnavailable"}}}}),
            ),
            (
                Error::CancellationUnresolved,
                UnsentCause::Refused(crate::NativeRefusalCode::NativeUnavailable),
                json!({"kind":"refused","value":{"reason":{"kind":"recipientRefused","value":{"code":"nativeUnavailable"}}}}),
            ),
            (
                Error::HostLocationsUnavailable {
                    location: "unresolvable host location".to_owned(),
                },
                UnsentCause::ProcessingFailure,
                json!({"kind":"refused","value":{"reason":{"kind":"processingFailed","value":{"kind":"executionFailed"}}}}),
            ),
        ] {
            // Act: real native emitter -> unchanged response translator -> SDK canonical mapper.
            let failure = emitter.recorded_policy_failure(
                &error,
                "resume",
                collaboration_protocol::NativeDeliveryFailureCause::RecordedResumePreparationFailed,
            );
            let delivery = interpret_native_response(
                &target(),
                crate::native_message_dispatch::NativeMessageOutcome::Failed(failure),
                false,
                &mut effects(),
            );
            assert_eq!(delivery.unsent_cause(), Some(expected_cause));
            let answer = crate::mail_push_outcome::local_message_outcome(delivery, true).unwrap();
            // Assert: an unpinned request still cannot hold these definite old public rejections.
            assert_eq!(
                serde_json::to_value(answer.outcome).unwrap(),
                expected_canonical
            );
            assert_eq!(
                serde_json::to_value(answer.observation.receipt).unwrap(),
                serde_json::to_value(&expected_receipt).unwrap()
            );
            assert!(answer.observation.scheduled_retry_at.is_none());
            assert!(answer.observation.route_evidence.is_none());
        }
    }

    #[test]
    fn codex_failure_families_reach_distinct_canonical_outcomes_through_the_real_translator() {
        let refused = |code: &str| json!({"kind":"refused","value":{"reason":{"kind":"recipientRefused","value":{"code":code}}}});
        let unknown_recipient =
            json!({"kind":"refused","value":{"reason":{"kind":"recipientUnknown"}}});
        let emitter = MessageEffects::new(json!(1));
        for (kind, allow_hold, expected) in [
            (
                NativeFailureKind::WrongService,
                true,
                json!({"kind":"refused","value":{"reason":{"kind":"processingFailed","value":{"kind":"invalidPayload"}}}}),
            ),
            (
                NativeFailureKind::EndpointNotFound,
                true,
                unknown_recipient.clone(),
            ),
            (
                NativeFailureKind::ThreadMissingOrUnmaterialized,
                true,
                unknown_recipient,
            ),
            (
                NativeFailureKind::UnsupportedCapability,
                true,
                refused("unsupportedCapability"),
            ),
            (
                NativeFailureKind::Unavailable,
                true,
                json!({"kind":"held","value":{"until":null}}),
            ),
            (
                NativeFailureKind::Unavailable,
                false,
                refused("nativeUnavailable"),
            ),
            (
                NativeFailureKind::StaleGeneration,
                true,
                refused("staleGeneration"),
            ),
            (
                NativeFailureKind::NotLoaded,
                true,
                json!({"kind":"held","value":{"until":null}}),
            ),
            (
                NativeFailureKind::NotLoaded,
                false,
                refused("threadNotLoaded"),
            ),
            (
                NativeFailureKind::ThreadNotLoaded,
                true,
                refused("threadNotLoaded"),
            ),
            (
                NativeFailureKind::NoActiveTurn,
                true,
                refused("noActiveTurn"),
            ),
            (
                NativeFailureKind::OutcomeUnknown,
                true,
                json!({"kind":"uncertain"}),
            ),
        ] {
            // Arrange and act: native emitter -> receipt translator -> canonical SDK result.
            let delivery = interpret_native_response(
                &target(),
                crate::native_message_dispatch::NativeMessageOutcome::Failed(
                    emitter.failure(kind, "nativeCall"),
                ),
                false,
                &mut effects(),
            );
            let receipt = serde_json::to_value(delivery.receipt()).unwrap();
            let answer =
                crate::mail_push_outcome::local_message_outcome(delivery, allow_hold).unwrap();
            // Assert: an unavailable native backend is not a missing recipient or a processing defect.
            assert_eq!(
                serde_json::to_value(answer.outcome).unwrap(),
                expected,
                "{kind:?}"
            );
            assert_eq!(
                serde_json::to_value(answer.observation.receipt).unwrap(),
                receipt
            );
            assert!(answer.observation.scheduled_retry_at.is_none());
            assert!(answer.observation.route_evidence.is_none());
        }
    }

    #[tokio::test]
    async fn malformed_native_read_keeps_processing_cause_and_publishes_local_rejection() {
        // Arrange and act through the existing socket endpoint, dispatch and real response translator.
        let (target, response) =
            crate::native_recorded_permission_tests::malformed_thread_read_probe().await;
        let crate::native_message_dispatch::NativeMessageOutcome::Failed(failure) = &response
        else {
            panic!("malformed native read was accepted");
        };
        assert!(matches!(
            failure.disposition,
            NativeFailureDisposition::Unsent(UnsentCause::ProcessingFailure)
        ));
        let mut evidence = effects();
        evidence.target = Some(target.clone());
        let delivery = interpret_native_response(&target, response, false, &mut evidence);
        assert_eq!(
            delivery.unsent_cause(),
            Some(UnsentCause::ProcessingFailure)
        );
        // The real emitter retains the processing cause beside main's released public rejection.
        let expected = collaboration_protocol::DeliveryReceipt {
            outcome: DeliveryOutcome::Rejected(DeliveryRejection {
                native_failure: None,
                reason: DeliveryRejectionReason::LocalRejection,
                next_action: DeliveryNextAction::InspectTarget,
                client_code: None,
                detail: None,
                claims: None,
            }),
            reachability: Some(SessionReachability::CodexAppServer),
            client: None,
        };
        let answer = crate::mail_push_outcome::local_message_outcome(delivery, true).unwrap();
        assert_eq!(
            serde_json::to_value(answer.observation.receipt).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert!(answer.observation.scheduled_retry_at.is_none());
        assert!(answer.observation.route_evidence.is_none());
        assert_eq!(
            serde_json::to_value(answer.outcome).unwrap(),
            json!({
                "kind":"refused","value":{"reason":{"kind":"processingFailed","value":{"kind":"executionFailed"}}}
            })
        );
    }
}
