//! The API's admission overload, answered in each family's own failure.
//!
//! A listener at its request limit answers a tool call with one generic payload,
//! `{kind: "overloaded", stage: "admission", effect: "none", message}`: the call did not
//! run, and the same operation identity may be retried. The client recognises it before any
//! family decode ([`ClientError::Overloaded`]), and each family client reports it as its own
//! typed `overloaded` failure with the next action "retry later", as the families always
//! have.
use crate::ClientError;
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, NonEmptyText, ProviderOperationEffect,
};
use serde_json::Value;

/// The overload a tool failure carries, if it is one.
pub(crate) fn admission_overload(failure: &Value) -> Option<ClientError> {
    let text = |field: &str| failure.get(field).and_then(Value::as_str);
    (text("kind") == Some("overloaded") && text("effect") == Some("none")).then(|| {
        ClientError::Overloaded {
            message: text("message")
                .filter(|message| !message.is_empty())
                .unwrap_or(DEFAULT_MESSAGE)
                .to_owned(),
        }
    })
}

const DEFAULT_MESSAGE: &str =
    "Request capacity exceeded; this request was not run. Retry when capacity is available.";

/// The conversation family's overload, for the CLI's typed conversation report.
#[must_use]
pub fn conversation(message: &str) -> Option<ConversationOperationFailure> {
    Some(ConversationOperationFailure {
        kind: ConversationOperationFailureKind::Overloaded,
        stage: ConversationOperationFailureStage::Admission,
        effect: ProviderOperationEffect::None,
        message: NonEmptyText::try_from(message.to_owned()).ok()?,
        operation_id: None,
        invalid_setting: None,
        provider_code: None,
        target: None,
        endpoint: None,
        availability: None,
    })
}
