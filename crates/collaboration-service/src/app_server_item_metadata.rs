//! Identify retained provider metadata that has no Codex transcript item.

use serde::Deserialize;
use session_event_model::{SessionItem, SessionItemKind};

/// ACP commands currently share Notice with human-facing provider notices.
/// Recognize only the complete serialized catalogue shape produced by
/// acp-client-runtime; arbitrary notices and assistant JSON remain content.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AvailableCommandsNotice {
    #[serde(rename = "availableCommands")]
    commands: Vec<AvailableCommand>,
    #[serde(rename = "_meta")]
    _metadata: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AvailableCommand {
    #[serde(rename = "name")]
    name: String,
    #[serde(rename = "description")]
    _description: String,
    #[serde(rename = "input")]
    _input: Option<CommandInput>,
    #[serde(rename = "_meta")]
    _metadata: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandInput {
    #[serde(rename = "hint")]
    _hint: String,
    #[serde(rename = "_meta")]
    _metadata: Option<serde::de::IgnoredAny>,
}

pub(crate) fn is_provider_metadata(item: &SessionItem) -> bool {
    match item.kind {
        SessionItemKind::Usage
        | SessionItemKind::ModeChange
        | SessionItemKind::ConfigChange
        | SessionItemKind::SessionInfo => true,
        SessionItemKind::Notice => item
            .text
            .as_deref()
            .is_some_and(|text| serde_json::from_str::<AvailableCommandsNotice>(text).is_ok()),
        SessionItemKind::UserMessage
        | SessionItemKind::AgentMessage
        | SessionItemKind::AgentThought
        | SessionItemKind::ToolCall { .. }
        | SessionItemKind::Plan
        | SessionItemKind::Unknown { .. } => false,
    }
}

/// Read one complete retained ACP replacement list, without interpreting prose.
#[must_use]
pub fn retained_available_command_names(item: &SessionItem) -> Option<Vec<String>> {
    if item.kind != SessionItemKind::Notice {
        return None;
    }
    serde_json::from_str::<AvailableCommandsNotice>(item.text.as_deref()?)
        .ok()
        .map(|notice| {
            notice
                .commands
                .into_iter()
                .map(|command| command.name)
                .collect()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_complete_command_catalogue_notice_is_metadata() {
        let cases = [
            (r#"{"availableCommands":[]}"#, true),
            (
                r#"{"availableCommands":[{"name":"compact","description":"Compact context","input":{"hint":"instructions"},"_meta":{"vendor":1}}],"_meta":{}}"#,
                true,
            ),
            ("availableCommands changed", false),
            (r#"{"availableCommands":"diagnostic"}"#, false),
            (r#"{"availableCommands":[{"message":"diagnostic"}]}"#, false),
            (
                r#"{"availableCommands":[{"name":"compact","description":"help","input":{"other":"diagnostic"}}]}"#,
                false,
            ),
            (
                r#"{"availableCommands":[],"message":"A meaningful notice"}"#,
                false,
            ),
        ];
        for (text, expected) in cases {
            let item = SessionItem {
                item_id: "notice".into(),
                kind: SessionItemKind::Notice,
                text: Some(text.into()),
            };
            assert_eq!(is_provider_metadata(&item), expected, "{text}");
            let assistant = SessionItem {
                kind: SessionItemKind::AgentMessage,
                ..item
            };
            assert!(
                !is_provider_metadata(&assistant),
                "assistant content must not be classified by its text"
            );
        }
    }
}
