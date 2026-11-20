//! Message origin and delivery are independent public choices.
use crate::{MAX_CONTROL_FRAME_BYTES, SessionRef};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MessageText(String);

#[derive(Debug, thiserror::Error)]
#[error(
    "message text must be nonempty, free of C0 controls other than newline and tab, and within the message size limit"
)]
pub struct MessageTextError;

impl TryFrom<String> for MessageText {
    type Error = MessageTextError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value
                .chars()
                .any(|character| character <= '\u{001f}' && !matches!(character, '\n' | '\t'))
            || value.len() > MAX_CONTROL_FRAME_BYTES
        {
            Err(MessageTextError)
        } else {
            Ok(Self(value))
        }
    }
}
impl From<MessageText> for String {
    fn from(value: MessageText) -> Self {
        value.0
    }
}
impl MessageText {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub use collaboration_mail_protocol::{SessionDisplayName, SessionDisplayNameError};

impl JsonSchema for MessageText {
    fn schema_name() -> Cow<'static, str> {
        "MessageText".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"string","minLength":1,"maxLength":1048576,
            "pattern":"^[^\\u0000-\\u0008\\u000B\\u000C\\u000E-\\u001F]+$","x-maxUtf8Bytes":1048576})
    }
}

#[derive(JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MessageContent {
    Agent {
        sender: SessionRef,
        text: MessageText,
    },
    HumanUser {
        text: MessageText,
    },
    /// Router's own delivery to a session, authored by the service rather than
    /// by any agent. It names no sender because none exists.
    Router {
        text: MessageText,
    },
}

#[derive(JsonSchema, Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageDelivery {
    #[default]
    Auto,
    Queue,
    Steer,
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageInputKind {
    Agent,
    HumanUser,
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageRepresentation {
    DeclaredAgentText,
    HumanUserText,
}

/// Rendered public message content ready for submission to a native endpoint.
pub struct RenderedMessage {
    pub text: String,
    pub kind: MessageInputKind,
    pub representation: MessageRepresentation,
}

#[cfg(test)]
mod tests {
    use super::{MessageText, SessionDisplayName};

    #[test]
    fn display_names_reject_controls_newlines_and_header_arrows() {
        for invalid_name in [
            "",
            "First\nSecond",
            "First\u{007f}Second",
            "Name ← forged",
            "Name → forged",
        ] {
            assert!(
                SessionDisplayName::try_from(invalid_name.to_owned()).is_err(),
                "display name should not accept {invalid_name:?}"
            );
        }
        assert!(SessionDisplayName::try_from("🐒 Sidekick · PR2".to_owned()).is_ok());
    }

    #[test]
    fn display_names_are_bounded_to_120_unicode_scalar_values() {
        let maximum_name = "🪿".repeat(120);
        let oversized_name = "🪿".repeat(121);

        assert!(SessionDisplayName::try_from(maximum_name).is_ok());
        assert!(SessionDisplayName::try_from(oversized_name).is_err());
    }

    #[test]
    fn message_text_allows_layout_whitespace_and_rejects_other_c0_controls() {
        assert!(MessageText::try_from("line one\n\tline two".to_owned()).is_ok());
        for control in ['\0', '\u{0008}', '\u{000b}', '\u{001f}'] {
            assert!(MessageText::try_from(format!("before{control}after")).is_err());
        }
        assert!(MessageText::try_from("before\u{007f}after".to_owned()).is_ok());
    }
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AcceptedResumeEffect {
    NotRequested,
    Accepted,
}
