use super::operation_failure_from_client_error;
use crate::ClientError;
use collaboration_protocol::{OperationEffect, OperationFailureKind};
use serde_json::json;

#[test]
fn decided_mail_refusals_have_no_effect_after_submission() {
    for kind in ["itemConflict", "invalidShape", "mailboxFull", "tooOld"] {
        let failure = operation_failure_from_client_error(
            ClientError::Rejected {
                code: -32602,
                data: Some(json!({"kind":kind,"stage":"discovery","message":"Decided refusal"})),
            },
            OperationEffect::Unknown,
        );
        assert_eq!(failure.effect, OperationEffect::None, "{kind}");
    }
}

#[test]
fn every_decided_wake_refusal_has_no_effect_after_submission() {
    for reason in [
        json!({"kind":"wakeExists","wakeup":"019f0000-0000-7000-8000-000000000101","creator":{"kind":"owner"}}),
        json!({"kind":"tooFarAhead","max":86400}),
        json!({"kind":"intervalTooShort","min":300}),
        json!({"kind":"lifetimeTooLong","max":86400}),
        json!({"kind":"expiryRequired"}),
    ] {
        let mut data = reason.clone();
        let fields = data.as_object_mut().expect("literal object");
        fields.extend(json!({"stage":"admission","message":"Decided refusal","operationId":null,"wakeupId":null,"effects":{"kind":"local","mutation":"none"},"nextAction":"correctRequest"}).as_object().expect("literal object").clone());
        let failure = operation_failure_from_client_error(
            ClientError::Rejected {
                code: -32050,
                data: Some(data),
            },
            OperationEffect::Unknown,
        );
        assert_eq!(failure.effect, OperationEffect::None, "{reason}");
    }
}

#[test]
fn uncertain_transport_and_unrecognized_refusals_keep_unknown_effect() {
    for source in [
        ClientError::Timeout,
        ClientError::Transport(std::io::Error::other("answer lost")),
        ClientError::Protocol("invalid mutation receipt"),
        ClientError::Rejected {
            code: -32050,
            data: Some(json!({"kind":"unavailable","stage":"store"})),
        },
        ClientError::Rejected {
            code: -32602,
            data: Some(json!({"kind":"futureRefusal","stage":"discovery"})),
        },
    ] {
        assert_eq!(
            operation_failure_from_client_error(source, OperationEffect::Unknown).effect,
            OperationEffect::Unknown
        );
    }
}

#[test]
fn preserves_service_kind_stage_code_and_data() {
    let failure = operation_failure_from_client_error(
        ClientError::Rejected {
            code: -32050,
            data: Some(json!({"kind":"staleGeneration","stage":"discovery","expected":2})),
        },
        OperationEffect::None,
    );
    assert_eq!(failure.kind, OperationFailureKind::Rejected);
    assert_eq!(failure.service_kind.as_deref(), Some("staleGeneration"));
    assert_eq!(failure.stage, "discovery");
    assert_eq!(failure.code, Some(-32050));
    assert_eq!(
        failure
            .data
            .as_ref()
            .and_then(|value| value.get("expected")),
        Some(&json!(2))
    );
}

#[test]
fn discovery_failure_keeps_structured_io_category() {
    let failure = operation_failure_from_client_error(
        ClientError::Discovery {
            stage: "manifest-read",
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "private path"),
        },
        OperationEffect::None,
    );
    assert_eq!(failure.kind, OperationFailureKind::Unavailable);
    assert_eq!(failure.stage, "manifest-read");
    assert_eq!(
        failure.data,
        Some(json!({"ioKind":"NotFound","osCode":null}))
    );
    assert!(!failure.message.contains("private path"));
}

#[test]
fn native_rejections_keep_the_server_message_and_do_not_claim_an_unknown_effect() {
    let failure = operation_failure_from_client_error(
        ClientError::Rejected {
            code: -32050,
            data: Some(json!({
                "kind":"unsupportedCapability",
                "stage":"rename",
                "message":"Codex app-server method `thread/name/set` is missing"
            })),
        },
        OperationEffect::Unknown,
    );

    assert_eq!(
        failure.message,
        "Codex app-server method `thread/name/set` is missing"
    );
    assert_eq!(failure.effect, OperationEffect::None);
}

#[test]
fn outcome_unknown_rejections_keep_the_server_message_and_unknown_effect() {
    let failure = operation_failure_from_client_error(
        ClientError::Rejected {
            code: -32050,
            data: Some(json!({
                "kind":"outcomeUnknown",
                "stage":"rename",
                "message":"native request outcome is unknown after dispatch"
            })),
        },
        OperationEffect::Unknown,
    );

    assert_eq!(
        failure.message,
        "native request outcome is unknown after dispatch"
    );
    assert_eq!(failure.effect, OperationEffect::Unknown);
}

#[test]
fn conversation_busy_rejections_keep_the_possible_effect_classification() {
    let failure = operation_failure_from_client_error(
        ClientError::Rejected {
            code: -32050,
            data: Some(json!({"kind":"busy","stage":"load"})),
        },
        OperationEffect::Unknown,
    );

    assert_eq!(failure.effect, OperationEffect::Unknown);
}
