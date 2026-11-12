//! Tool results shared by every tool: operation failures and the client errors the carrier
//! tools still meet.
use super::{
    CallToolResponse, CallToolResult, ClientError, McpToolError, McpToolOutput, OperationEffect,
    operation_failure_from_client_error, operation_failure_result, success_result,
};

/// A carrier tool's typed result, or its client error as an operation failure.
pub(super) fn structured_result<TValue: serde::Serialize>(
    result: Result<TValue, ClientError>,
    possible_effect: OperationEffect,
) -> CallToolResult {
    match result {
        Ok(value) => success_result(&value),
        Err(error) => failure(error, possible_effect),
    }
}

pub(crate) fn structured_tool_error(value: serde_json::Value) -> CallToolResult {
    let error =
        McpToolOutput::<serde_json::Value>::Error(Box::new(McpToolError::from_existing(value)));
    match serde_json::to_value(error) {
        Ok(structured) => CallToolResult::structured_error(structured),
        Err(_) => CallToolResult::error(vec![rmcp::model::ContentBlock::text(
            "MCP tool error encoding failed",
        )]),
    }
}

pub(super) fn operation_error_result(
    failure: collaboration_protocol::AdapterOperationFailure,
    target: Option<collaboration_protocol::SessionRef>,
    turn_id: Option<collaboration_protocol::NonEmptyText>,
) -> CallToolResult {
    serde_json::to_value(failure)
        .map(|mut value| {
            if let Some(fields) = value.as_object_mut() {
                fields.insert("target".to_owned(), serde_json::json!(target));
                fields.insert("turnId".to_owned(), serde_json::json!(turn_id));
            }
            structured_tool_error(value)
        })
        .unwrap_or_else(|_| validation_failure("collaboration error encoding failed"))
}

pub(super) fn failure(error: ClientError, possible_effect: OperationEffect) -> CallToolResult {
    operation_failure_result(&operation_failure_from_client_error(error, possible_effect))
}

pub(super) fn validation_failure(message: &str) -> CallToolResult {
    structured_tool_error(serde_json::json!({
        "kind": "protocolViolation",
        "stage": "validation",
        "effect": "none",
        "message": message,
        "code": null,
        "data": null
    }))
}

/// rmcp answers arguments that do not decode into a tool's parameters with a text-only error,
/// before the handler runs; it is the one tool error the handlers do not build themselves.
/// It becomes the structured validation failure every other refusal before dispatch already
/// is, so a client always reads a typed result. Nothing was dispatched, so nothing took effect.
pub(super) fn structured_argument_refusal(response: CallToolResponse) -> CallToolResponse {
    match response {
        CallToolResponse::Complete(result)
            if result.is_error == Some(true) && result.structured_content.is_none() =>
        {
            let message = result
                .content
                .iter()
                .filter_map(|content| content.as_text().map(|text| text.text.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            CallToolResponse::Complete(validation_failure(if message.is_empty() {
                "The tool arguments do not match the tool's input schema"
            } else {
                &message
            }))
        }
        response => response,
    }
}
