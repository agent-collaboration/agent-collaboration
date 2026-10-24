use super::super::operation_failure_exit;
use super::receipt_exit_status;
use collaboration_client::{ClientError, MessageSendError, OperationEffect};

#[test]
fn queued_report_names_sender_custody_without_a_push_or_native_receipt() {
    let queued = serde_json::from_value(serde_json::json!({
        "item":"01900000-0000-7000-8000-000000000002",
        "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000099","endpointId":"codex-local"},"sessionId":"remote-recipient"},
        "targetIdentity":"remote recipient"
    })).unwrap();
    let (record, code, confirmation, failure, write_failure) = super::queued_report(queued);
    assert_eq!(code, 0);
    assert_eq!(
        write_failure, 5,
        "a lost answer must not invite an unsafe resend"
    );
    assert!(failure.is_none());
    let confirmation = confirmation.unwrap();
    assert!(
        confirmation
            .starts_with("queued item 01900000-0000-7000-8000-000000000002 to remote recipient ")
    );
    let saved = &record["result"]["record"];
    assert_eq!(saved["queued"]["targetIdentity"], "remote recipient");
    for field in ["pushId", "link", "receipt", "deliveryState"] {
        assert!(
            saved.get(field).is_none(),
            "queued custody invented {field}"
        );
    }
}

#[test]
fn cli_receipt_keeps_foreign_writer_reason_action_and_rejected_exit() {
    let receipt: collaboration_client::protocol::DeliveryReceipt =
        serde_json::from_value(serde_json::json!({
            "outcome":{
                "kind":"rejected","reason":"heldByAnotherClient",
                "nextAction":"messageFromHoldingCodexClient","clientCode":-32600,
                "detail":"Message it from the Codex client that holds it."
            },
            "reachability":"codexAppServer","client":null
        }))
        .expect("typed receipt");
    assert_eq!(
        receipt_exit_status(collaboration_client::protocol::PushDeliveryState::Rejected),
        4
    );
    let output = crate::endpoint_commands::result_envelope(serde_json::json!(receipt));
    assert_eq!(
        output["result"]["record"]["outcome"]["reason"],
        "heldByAnotherClient"
    );
    assert_eq!(
        output["result"]["record"]["outcome"]["nextAction"],
        "messageFromHoldingCodexClient"
    );
}

#[test]
fn adapter_distinguishes_preparation_loss_from_post_dispatch_loss() {
    let transport = || {
        ClientError::Transport(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "fixture",
        ))
    };
    let (preparation, preparation_target) =
        MessageSendError::Preparation(Box::new(transport())).into_operation_failure_and_target();
    assert!(preparation_target.is_none());
    assert_eq!(preparation.effect, OperationEffect::None);
    assert_eq!(
        preparation.kind,
        collaboration_client::OperationFailureKind::Unavailable
    );

    let (submission, submission_target) = MessageSendError::Submission {
        target: serde_json::from_value(serde_json::json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
            "sessionId":"target"
        }))
        .expect("target"),
        source: Box::new(transport()),
    }
    .into_operation_failure_and_target();
    assert!(submission_target.is_some());
    assert_eq!(submission.effect, OperationEffect::Unknown);
    assert_eq!(
        submission.kind,
        collaboration_client::OperationFailureKind::Unavailable
    );

    let (malformed_after_dispatch, malformed_target) = MessageSendError::Submission {
        target: serde_json::from_value(serde_json::json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
            "sessionId":"target"
        }))
        .expect("target"),
        source: Box::new(ClientError::Protocol("malformed result")),
    }
    .into_operation_failure_and_target();
    assert!(malformed_target.is_some());
    assert_eq!(malformed_after_dispatch.effect, OperationEffect::Unknown);
    assert_eq!(operation_failure_exit(&malformed_after_dispatch), 5);
}

#[test]
fn adapter_preserves_established_service_rejection_exit_codes() {
    for (service_kind, expected) in [
        ("outcomeUnknown", 5),
        ("unsupportedCapability", 2),
        ("unavailable", 3),
        ("nativeRejected", 4),
    ] {
        let (failure, _) = MessageSendError::Preparation(Box::new(ClientError::Rejected {
            code: -32050,
            data: Some(serde_json::json!({"kind":service_kind})),
        }))
        .into_operation_failure_and_target();
        assert_eq!(operation_failure_exit(&failure), expected, "{service_kind}");
    }
}
