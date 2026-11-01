//! Malformed raw wire values must keep the named product-limit refusal.
use super::raw_call;
use crate::{ConformanceResult, native_board_session::NativeBoardSession};
use collaboration_protocol::{
    AutomationAnswer, AutomationRefusal, AutomationValidationError,
    limits::{INSTRUCTION_MAX_BYTES, LimitName, WAKE_MESSAGE_MAX_BYTES},
};
use serde_json::{Value, json};

pub(super) async fn run(machine: &mut NativeBoardSession, service: &str) -> ConformanceResult<()> {
    let operation = durable_steps::Uuid::now_v7().to_string();
    let instruction =
        json!({"kind":"instructionCreate","value":{"operationId":operation,"text":"ok"}});
    let _: collaboration_protocol::AutomationCall = serde_json::from_value(instruction.clone())?;
    for text in [String::new(), "🦀".repeat(INSTRUCTION_MAX_BYTES / 4 + 1)] {
        let mut request = instruction.clone();
        *request
            .pointer_mut("/value/text")
            .ok_or("instruction text disappeared")? = json!(text);
        limited(
            machine,
            request,
            LimitName::InstructionMaxBytes,
            text.len() as u64,
        )
        .await?;
    }
    let wake = json!({"kind":"wakeSend","value":{
        "operationId":durable_steps::Uuid::now_v7().to_string(), "requestedBy":{"kind":"owner"}, "replace":false,
        "message":{"target":{"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"target"},
            "content":{"kind":"humanUser","text":"wake"}, "delivery":"auto"},
        "timing":{"kind":"at","at":"2026-10-31T10:05:00Z"}, "expiry":{"kind":"none"}
    }});
    let _: collaboration_protocol::AutomationCall = serde_json::from_value(wake.clone())?;
    for (pointer, text, limit) in [
        (
            "/value/message/content/text",
            "🦀".repeat(WAKE_MESSAGE_MAX_BYTES / 4 + 1),
            LimitName::WakeMessageMaxBytes,
        ),
        (
            "/value/message/target/sessionId",
            "🦀".repeat(1025),
            LimitName::SessionIdMaxBytes,
        ),
        (
            "/value/message/target/endpoint/endpointId",
            "a".repeat(65),
            LimitName::EndpointIdMaxBytes,
        ),
    ] {
        let mut request = wake.clone();
        *request
            .pointer_mut(pointer)
            .ok_or("raw limit pointer disappeared")? = json!(text);
        limited(machine, request, limit, text.len() as u64).await?;
    }
    let mut report = json!({"kind":"reportWorkItems","value":{"reports":[{"kind":"firing","value":{
        "firingId":durable_steps::Uuid::now_v7().to_string(), "seq":1,
        "outcome":{"state":"failed","reason":"bounded reason"}
    }}]}});
    let _: collaboration_protocol::AutomationCall = serde_json::from_value(report.clone())?;
    for counter in [0, collaboration_protocol::limits::JSON_SAFE_INTEGER_MAX + 1] {
        let mut request = report.clone();
        *request
            .pointer_mut("/value/reports/0/value/seq")
            .ok_or("report sequence disappeared")? = json!(counter);
        limited(machine, request, LimitName::JsonSafeInteger, counter).await?;
    }
    let reason = "🦀".repeat(1025);
    *report
        .pointer_mut("/value/reports/0/value/outcome/reason")
        .ok_or("report reason disappeared")? = json!(reason);
    limited(
        machine,
        report,
        LimitName::TextMaxBytes,
        reason.len() as u64,
    )
    .await?;
    Ok(())
}

async fn limited(
    machine: &mut NativeBoardSession,
    request: Value,
    expected: LimitName,
    value: u64,
) -> ConformanceResult<()> {
    match raw_call(machine, request).await? {
        AutomationAnswer::Refused(AutomationRefusal::Validation {
            error: AutomationValidationError::LimitExceeded(error),
        }) if error.limit == expected
            && error.value == value
            && error.minimum == expected.definition().minimum
            && error.maximum == expected.definition().maximum
            && error.step == expected.definition().step =>
        {
            Ok(())
        }
        other => Err(format!("raw {expected} violation lost its typed limit: {other:?}").into()),
    }
}
