//! The advertised output schemas accept the results tools build from real domain values:
//! classified Codex session failures and lifecycle records decoded from the journal's wire
//! form. The hand-written samples for every tool live in `schema_tests.rs`.
use super::*;
use collaboration_service::collaboration_application::{
    NativeSessionFailure, NativeSessionFailureKind, NativeSessionStage,
};

pub(super) fn advertised_output_validator(tool_name: &str) -> jsonschema::Validator {
    let schema = CollaborationMcpServer::catalog_only()
        .resolved_tools()
        .into_iter()
        .find(|tool| tool.name == tool_name)
        .and_then(|tool| tool.output_schema)
        .unwrap_or_else(|| panic!("{tool_name} advertises an output schema"));
    jsonschema::validator_for(&Value::Object((*schema).clone()))
        .unwrap_or_else(|error| panic!("{tool_name} output validator: {error}"))
}

pub(super) fn validated_structured_content(
    validator: &jsonschema::Validator,
    tool_name: &str,
    result: rmcp::model::CallToolResult,
) -> Value {
    let structured = result
        .structured_content
        .unwrap_or_else(|| panic!("{tool_name} result is structured"));
    if let Err(error) = validator.validate(&structured) {
        panic!("{tool_name} advertised schema rejects its own result: {error}; {structured}");
    }
    structured
}

#[test]
fn session_inspect_and_rename_schemas_accept_every_classified_failure_they_emit() {
    let native_rejection = |stage, message: &str, reason, next_action, native_code| {
        NativeSessionFailure::NativeRejected {
            stage,
            message: message.to_owned(),
            reason,
            next_action,
            native_code,
        }
    };
    for (tool_name, stage, possible_effect) in [
        (
            "session_inspect",
            NativeSessionStage::Inspect,
            OperationEffect::None,
        ),
        (
            "session_rename",
            NativeSessionStage::Rename,
            OperationEffect::Unknown,
        ),
    ] {
        // Arrange: one failure per class the classifier hands these tools.
        let validator = advertised_output_validator(tool_name);
        let mut failures = vec![
            native_rejection(
                stage,
                "thread has an active turn and is busy",
                collaboration_protocol::DeliveryRejectionReason::Busy,
                collaboration_protocol::DeliveryNextAction::UseDeliverySteer,
                None,
            ),
            native_rejection(
                stage,
                "Native control operation failed",
                collaboration_protocol::DeliveryRejectionReason::Unknown,
                collaboration_protocol::DeliveryNextAction::RetryLater,
                Some(-32099),
            ),
            native_rejection(
                stage,
                "thread 019f0000-0000-7000-8000-000000000001 already has an active writer",
                collaboration_protocol::DeliveryRejectionReason::HeldByAnotherClient,
                collaboration_protocol::DeliveryNextAction::MessageFromHoldingCodexClient,
                None,
            ),
            NativeSessionFailure::Refused {
                kind: NativeSessionFailureKind::Unavailable,
                stage,
                message: "Native control operation failed".to_owned(),
            },
        ];
        if stage == NativeSessionStage::Rename {
            failures.push(NativeSessionFailure::NameMismatch {
                requested: "Review".to_owned(),
                effective: "Old name".to_owned(),
            });
        }

        for failure in failures {
            let classification = serde_json::to_value(&failure).expect("failure encodes");

            // Act
            let result =
                super::super::application_result::<Value, _>(Err(failure), possible_effect);

            // Assert: the schema admits the error, and the error keeps the classification
            // (reason, corrective action, native code, requested and echoed names).
            assert_eq!(result.is_error, Some(true), "{tool_name} reports an error");
            let structured = validated_structured_content(&validator, tool_name, result);
            assert_eq!(
                structured["data"], classification,
                "{tool_name} error keeps the classified failure"
            );
        }
    }
}

#[test]
fn journal_read_schema_accepts_a_page_of_decoded_flattened_lifecycle_records() {
    // Arrange: a page decoded from the journal's wire form, so each record is the flattened
    // observation the journal actually stores.
    let validator = advertised_output_validator("journal_read");
    let id = FIXTURE_SERVICE_ID;
    let page: collaboration_protocol::JournalPage = serde_json::from_value(serde_json::json!({
        "bounds":{"journalId":id,"earliestSequence":1,"lastSequence":1},
        "records":[{
            "journalId":id,"sequence":1,"schemaVersion":1,"observedAt":"2026-09-05T12:00:00Z",
            "source":"observerLifecycle",
            "scope":{"endpoint":{"serviceId":id,"endpointId":"codex-local"},"generation":null,"observerId":id},
            "subject":{"kind":"backend"},"change":{"kind":"coverageLost"}
        }],
        "next":{"journalId":id,"sequence":1},"caughtUp":true
    }))
    .expect("journal page decodes");

    // Act
    let result = super::super::success_result(&page);

    // Assert
    let structured = validated_structured_content(&validator, "journal_read", result);
    assert_eq!(structured["records"][0]["source"], "observerLifecycle");
}
