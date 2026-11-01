//! Preserve typed limit errors before Serde reports other malformed wire shapes.
use crate::automation_store_error::{AutomationStoreError, AutomationStoreResult};
use board_sync_protocol::BoardPayload;
use collaboration_protocol::{AutomationCall, AutomationRequestConstraint, limits::LimitName};
use serde_json::Value;

pub(crate) fn decode(payload: BoardPayload) -> AutomationStoreResult<AutomationCall> {
    let value: Value = payload.decode().map_err(|_| malformed())?;
    call_limits(&value)?;
    serde_json::from_value(value).map_err(|_| malformed())
}

fn malformed() -> AutomationStoreError {
    AutomationStoreError::InvalidRequest {
        field: "call",
        reason: AutomationRequestConstraint::MalformedShape,
    }
}

pub(crate) fn call_limits(call: &Value) -> AutomationStoreResult<()> {
    identity_limits(call)?;
    let value = &call["value"];
    match call["kind"].as_str() {
        Some("instructionCreate" | "instructionUpdate") => {
            text(&value["text"], LimitName::InstructionMaxBytes)?
        }
        Some("wakeSend") => {
            message_limits(at(value, "/message/content"))?;
            counter_limits(at(value, "/message/generationGuard"))?;
        }
        Some("remoteSessionCreate") => message_limits(at(value, "/definition/firstInput"))?,
        Some("reportWorkItems") => {
            if let Some(reports) = value["reports"].as_array() {
                LimitName::ReportFlushMax.validate(reports.len() as u64)?;
                for report in reports {
                    let report = &report["value"];
                    counter_limits(report)?;
                    outcome_limits(&report["outcome"])?;
                    text(at(report, "/input/reason"), LimitName::TextMaxBytes)?;
                }
            }
        }
        Some("localRunSettled") => outcome_limits(&value["outcome"])?,
        _ => {}
    }
    Ok(())
}

/// Every endpointId and sessionId in these DTOs names the same validated scalar.
/// Serde still checks the complete shape after this lossless length preflight.
pub(crate) fn identity_limits(value: &Value) -> AutomationStoreResult<()> {
    match value {
        Value::Object(fields) => {
            for (name, child) in fields {
                match name.as_str() {
                    "endpointId" => text(child, LimitName::EndpointIdMaxBytes)?,
                    "sessionId" => text(child, LimitName::SessionIdMaxBytes)?,
                    _ => identity_limits(child)?,
                }
            }
        }
        Value::Array(children) => {
            for child in children {
                identity_limits(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn counter_limits(value: &Value) -> AutomationStoreResult<()> {
    match value {
        Value::Object(fields) => {
            for (name, child) in fields {
                if matches!(name.as_str(), "seq" | "generation" | "reportSeq") {
                    if let Some(counter) = child.as_u64() {
                        LimitName::JsonSafeInteger.validate(counter)?;
                    }
                } else {
                    counter_limits(child)?;
                }
            }
        }
        Value::Array(children) => {
            for child in children {
                counter_limits(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn text(value: &Value, limit: LimitName) -> AutomationStoreResult<()> {
    if let Some(value) = value.as_str() {
        limit.validate(value.len() as u64)?;
    }
    Ok(())
}
fn message_limits(value: &Value) -> AutomationStoreResult<()> {
    text(&value["text"], LimitName::WakeMessageMaxBytes)
}
pub(crate) fn outcome_limits(value: &Value) -> AutomationStoreResult<()> {
    text(&value["reason"], LimitName::TextMaxBytes)?;
    text(&value["summary"], LimitName::InstructionMaxBytes)
}

pub(crate) fn answer_limits(answer: &Value) -> AutomationStoreResult<()> {
    identity_limits(answer)?;
    counter_limits(answer)?;
    let value = &answer["value"];
    match answer["kind"].as_str() {
        Some("instruction") => text(&value["text"], LimitName::InstructionMaxBytes)?,
        Some("instructions" | "revisions") => {
            if let Some(records) = value["records"].as_array() {
                for record in records {
                    text(&record["text"], LimitName::InstructionMaxBytes)?;
                }
            }
        }
        Some("wake") => message_limits(at(value, "/definition/message/content"))?,
        Some("firing") => outcome_limits(&value["outcome"])?,
        Some("mutation") => answer_limits(&value["automation"])?,
        Some("list") => {
            if let Some(automations) = value["automations"].as_array() {
                for automation in automations {
                    answer_limits(automation)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn at<'value>(value: &'value Value, pointer: &str) -> &'value Value {
    value.pointer(pointer).unwrap_or(&Value::Null)
}
