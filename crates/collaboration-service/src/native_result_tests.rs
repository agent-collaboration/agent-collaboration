//! Cross-layer native result tests retained for application integration.
#[cfg(test)]
mod tests {
    use super::*;
    use collaboration_runtime::message_effect_state::MessageEffects;
    use collaboration_runtime::message_effect_state::NativeFailureKind;
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
                collaboration_runtime::native_message_dispatch::NativeMessageOutcome::Failed(failure),
                false,
                &mut effects(),
            );
            assert_eq!(
                delivery.unsent_cause(),
                Some(UnsentCause::Refused(
                    collaboration_contracts::NativeRefusalCode::Rejection(
                        DeliveryRejectionReason::StaleGeneration
                    )
                ))
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
            collaboration_runtime::native_message_dispatch::NativeMessageOutcome::Failed(failure),
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
        let settings = UnsentCause::Refused(collaboration_contracts::NativeRefusalCode::Rejection(
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
                collaboration_runtime::native_message_dispatch::NativeMessageOutcome::Failed(failure),
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
                UnsentCause::Refused(collaboration_contracts::NativeRefusalCode::NativeUnavailable),
                json!({"kind":"refused","value":{"reason":{"kind":"recipientRefused","value":{"code":"nativeUnavailable"}}}}),
            ),
            (
                Error::CancellationUnresolved,
                UnsentCause::Refused(collaboration_contracts::NativeRefusalCode::NativeUnavailable),
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
                collaboration_runtime::native_message_dispatch::NativeMessageOutcome::Failed(failure),
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
                collaboration_runtime::native_message_dispatch::NativeMessageOutcome::Failed(
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
            collaboration_runtime::native_recorded_permission_tests::malformed_thread_read_probe().await;
        let collaboration_runtime::native_message_dispatch::NativeMessageOutcome::Failed(failure) = &response
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
