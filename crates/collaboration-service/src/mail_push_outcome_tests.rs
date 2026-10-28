//! Literal canonical outcomes challenge lossy booleans and human-reason inference.
use super::*;
use collaboration_contracts::NativeObservedOutcome;
use collaboration_protocol::{
    DeliveryNextAction, DeliveryRejection, DeliveryRejectionReason, SessionReachability,
};
use serde_json::{Value, json};

fn expected_coded_refusal(code: &str) -> Value {
    json!({"kind":"refused","value":{"reason":{"kind":"recipientRefused","value":{"code":code}}}})
}

#[test]
fn every_definite_rejection_keeps_its_literal_code_and_complete_local_receipt() {
    use DeliveryRejectionReason as Reason;
    let cases = [
        (Reason::ChildThread, Some("childThread")),
        (Reason::Busy, Some("busy")),
        (Reason::HeldByAnotherClient, Some("heldByAnotherClient")),
        (Reason::SettingsUnresolved, Some("settingsUnresolved")),
        (Reason::NotResumable, Some("notResumable")),
        (Reason::PermissionDenied, Some("permissionDenied")),
        (Reason::UnsupportedCapability, Some("unsupportedCapability")),
        (Reason::Unknown, Some("unknown")),
        (Reason::EndpointUnavailable, Some("endpointUnavailable")),
        (Reason::NoRoute, None),
        (Reason::LiveElsewhere, Some("liveElsewhere")),
        (Reason::SteerUnsupported, Some("steerUnsupported")),
        (Reason::QueueUnsupported, Some("queueUnsupported")),
        (Reason::StaleGeneration, Some("staleGeneration")),
        (Reason::ProviderSessionNotFound, None),
        (Reason::ProviderRejected, Some("providerRejected")),
    ];
    for (reason, code) in cases {
        for detail in ["original native explanation", "renamed explanation"] {
            // Arrange: the emitter's closed rejection classification, not its text.
            let delivery = NativeDeliveryResult::rejected(
                DeliveryRejection {
                    native_failure: None,
                    reason,
                    next_action: DeliveryNextAction::InspectTarget,
                    client_code: Some(47),
                    detail: Some(detail.to_owned()),
                    claims: None,
                },
                UnsentCause::for_rejection(reason),
            )
            .with_reachability(SessionReachability::ProviderAcp);
            let complete_receipt = serde_json::to_value(delivery.receipt()).unwrap();
            // Act: a hard request cannot turn an explicit rejection into a hold.
            let answer = local_message_outcome(delivery, false).unwrap();
            // Assert: Unknown rejection is definite; only the two named rows are unknown recipients.
            let expected = code.map_or_else(
                || json!({"kind":"refused","value":{"reason":{"kind":"recipientUnknown"}}}),
                expected_coded_refusal,
            );
            assert_eq!(serde_json::to_value(answer.outcome).unwrap(), expected);
            assert_eq!(
                serde_json::to_value(answer.observation.receipt).unwrap(),
                complete_receipt
            );
        }
    }
}

#[test]
fn every_not_submitted_code_uses_metadata_instead_of_retryable_or_human_text() {
    use NativeRefusalCode as Code;
    let cases = [
        (Code::RuntimeMissing, "runtimeMissing"),
        (Code::NativeUnavailable, "nativeUnavailable"),
        (Code::HeldSessionBusy, "heldSessionBusy"),
        (Code::ThreadNotLoaded, "threadNotLoaded"),
        (Code::NoActiveTurn, "noActiveTurn"),
        (Code::SteerFailed, "steerFailed"),
        (Code::ActivityUnavailable, "activityUnavailable"),
        (Code::ProviderNotSubmitted, "providerNotSubmitted"),
        (Code::PeerUnavailable, "peerUnavailable"),
        (
            Code::PeerAuthenticationUnavailable,
            "peerAuthenticationUnavailable",
        ),
        (Code::PeerEncodingFailed, "peerEncodingFailed"),
        (Code::PeerFrameTooLarge, "peerFrameTooLarge"),
        (Code::PeerConnectFailed, "peerConnectFailed"),
        (Code::PeerWriteNotStarted, "peerWriteNotStarted"),
        (Code::CarrierUnavailable, "carrierUnavailable"),
        (Code::QueuedNotSubmitted, "queuedNotSubmitted"),
    ];
    for (code, expected) in cases {
        for reason in [
            "original explanation",
            "staleGeneration is only human text here",
        ] {
            for retryable in [false, true] {
                // Arrange: this emitter classifies a definite refusal even if its legacy boolean differs.
                let delivery = NativeDeliveryResult::not_submitted(
                    reason,
                    retryable,
                    UnsentCause::Refused(code),
                );
                let complete_receipt = serde_json::to_value(delivery.receipt()).unwrap();
                // Act and assert: neither the boolean nor a misleading substring chooses policy.
                let answer = local_message_outcome(delivery, true).unwrap();
                assert_eq!(
                    serde_json::to_value(answer.outcome).unwrap(),
                    expected_coded_refusal(expected)
                );
                assert_eq!(
                    serde_json::to_value(answer.observation.receipt).unwrap(),
                    complete_receipt
                );
            }
        }
    }
}

#[test]
fn holdable_availability_holds_only_when_the_frozen_request_permits_it() {
    for allow_hold in [false, true] {
        // Arrange: one no-effect answer, with the emitter's hard-request fallback.
        let delivery = NativeDeliveryResult::not_submitted(
            "renamed backend availability detail",
            true,
            UnsentCause::Holdable {
                otherwise: NativeRefusalCode::NativeUnavailable,
            },
        );
        let complete_receipt = serde_json::to_value(delivery.receipt()).unwrap();
        // Act.
        let answer = local_message_outcome(delivery, allow_hold).unwrap();
        // Assert: a guard or steer is terminal without changing its full public receipt.
        let expected = if allow_hold {
            json!({"kind":"held","value":{"until":null}})
        } else {
            expected_coded_refusal("nativeUnavailable")
        };
        assert_eq!(serde_json::to_value(answer.outcome).unwrap(), expected);
        assert_eq!(
            serde_json::to_value(answer.observation.receipt).unwrap(),
            complete_receipt
        );
    }
}

#[test]
fn specifically_missing_recipient_and_genuine_defects_remain_distinct() {
    for (cause, expected) in [
        (
            UnsentCause::RecipientUnknown,
            json!({"kind":"refused","value":{"reason":{"kind":"recipientUnknown"}}}),
        ),
        (
            UnsentCause::InvalidInput,
            json!({"kind":"refused","value":{"reason":{"kind":"processingFailed","value":{"kind":"invalidPayload"}}}}),
        ),
        (
            UnsentCause::ProcessingFailure,
            json!({"kind":"refused","value":{"reason":{"kind":"processingFailed","value":{"kind":"executionFailed"}}}}),
        ),
    ] {
        // Arrange: all old public outcomes are identical.
        let delivery = NativeDeliveryResult::not_submitted("same human text", false, cause);
        // Act and assert: only the emitter's cause chooses the canonical result.
        let answer = local_message_outcome(delivery, true).unwrap();
        assert_eq!(serde_json::to_value(answer.outcome).unwrap(), expected);
    }
}

#[test]
fn accepted_native_outcomes_and_uncertainty_are_never_refusals() {
    for (native, expected) in [
        (
            NativeObservedOutcome::Started,
            json!({"kind":"delivered","value":{"acceptance":"started"}}),
        ),
        (
            NativeObservedOutcome::Steered,
            json!({"kind":"delivered","value":{"acceptance":"steered"}}),
        ),
        (
            NativeObservedOutcome::StartedOrSteered,
            json!({"kind":"delivered","value":{"acceptance":"startedOrSteered"}}),
        ),
        (
            NativeObservedOutcome::Queued,
            json!({"kind":"delivered","value":{"acceptance":"queued"}}),
        ),
        (
            NativeObservedOutcome::PeerMessageWritten,
            json!({"kind":"delivered","value":{"acceptance":"peerMessageWritten"}}),
        ),
        (NativeObservedOutcome::Unknown, json!({"kind":"uncertain"})),
    ] {
        // Arrange, act and assert through the actual SDK LocalOutcome codec.
        let answer =
            local_message_outcome(NativeDeliveryResult::observed(native, None, None), true)
                .unwrap();
        assert_eq!(serde_json::to_value(answer.outcome).unwrap(), expected);
    }
}
