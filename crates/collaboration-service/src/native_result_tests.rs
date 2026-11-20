//! Cross-layer native result tests retained for application integration.
#[cfg(test)]
mod tests {
    use agent_automation::CessationEvidence;
    use agent_automation::{NativeEffectEvidence, PreparationEffect, SubmissionEffect};
    use collaboration_contracts::UnsentCause;
    use collaboration_protocol::*;
    use collaboration_runtime::test_support::interpret_native_response;
    use collaboration_runtime::test_support::message_effect_state::MessageEffects;
    use collaboration_runtime::test_support::message_effect_state::NativeFailureKind;
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
                collaboration_runtime::test_support::native_message_dispatch::NativeMessageOutcome::Failed(failure),
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
        }
    }
    #[test]
    fn unknown_native_effect_cannot_become_holdable_from_an_unavailable_reason() {
        let mut emitter = MessageEffects::new(json!(1));
        emitter.submission = "unknown";
        let failure = emitter.failure(NativeFailureKind::Unavailable, "start");
        let delivery = interpret_native_response(
            &target(),
            collaboration_runtime::test_support::native_message_dispatch::NativeMessageOutcome::Failed(failure),
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
                emitter.failure(NativeFailureKind::LocalRejection(settings), "inspect"),
                settings,
            ),
        ] {
            let result = interpret_native_response(
                &target(),
                collaboration_runtime::test_support::native_message_dispatch::NativeMessageOutcome::Failed(failure),
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
}
