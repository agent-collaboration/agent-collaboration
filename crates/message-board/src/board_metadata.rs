use crate::{BoardId, ProjectId, WorkspaceId};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub const MAX_NAME_BYTES: usize = 256;
pub const MAX_DESCRIPTION_BYTES: usize = 16 * 1024;
pub const MAX_MESSAGE_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, thiserror::Error, Clone, Eq, PartialEq)]
#[error("{field} {requirement}")]
pub struct TextValidationError {
    pub field: &'static str,
    pub requirement: &'static str,
}

macro_rules! bounded_text {
    ($name:ident, $field:literal, $min:expr, $max:expr, $trim:expr, $reject_c0:expr) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> { stringify!($name).into() }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({"type":"string","minLength":$min,"maxLength":$max,"x-maxUtf8Bytes":$max})
            }
        }
        impl TryFrom<String> for $name {
            type Error = TextValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                let canonical = if $trim { value.trim().to_owned() } else { value };
                if !($min..=$max).contains(&canonical.len()) {
                    return Err(TextValidationError { field: $field, requirement: "is outside its UTF-8 byte bound" });
                }
                if $reject_c0 && canonical.chars().any(|character| character <= '\u{001f}' && !matches!(character, '\n' | '\t')) {
                    return Err(TextValidationError { field: $field, requirement: "contains a forbidden control character" });
                }
                Ok(Self(canonical))
            }
        }
        impl From<$name> for String { fn from(value: $name) -> Self { value.0 } }
        impl $name { #[must_use] pub fn as_str(&self) -> &str { &self.0 } }
    };
}

// name, minimum, maximum, trims surrounding whitespace, rejects C0 controls.
bounded_text!(ResourceName, "name", 1, MAX_NAME_BYTES, true, false);
bounded_text!(
    Description,
    "description",
    0,
    MAX_DESCRIPTION_BYTES,
    false,
    false
);
bounded_text!(PostText, "text", 1, MAX_MESSAGE_TEXT_BYTES, false, true);
bounded_text!(SearchQuery, "query", 1, MAX_NAME_BYTES, true, false);
bounded_text!(TaskTitle, "title", 1, MAX_NAME_BYTES, true, true);
bounded_text!(TrailText, "text", 1, MAX_MESSAGE_TEXT_BYTES, false, true);
bounded_text!(DropReason, "reason", 1, MAX_MESSAGE_TEXT_BYTES, false, true);
bounded_text!(TrailTag, "tag", 1, MAX_NAME_BYTES, true, true);
bounded_text!(MarkdownText, "text", 1, MAX_MESSAGE_TEXT_BYTES, false, true);
bounded_text!(MachineLabel, "machineLabel", 1, MAX_NAME_BYTES, true, true);

#[cfg(test)]
mod tests {
    use super::PostText;

    #[test]
    fn board_message_text_allows_layout_whitespace_and_rejects_other_c0_controls() {
        assert!(PostText::try_from("line one\n\tline two".to_owned()).is_ok());
        for control in ['\0', '\u{0008}', '\u{000b}', '\u{001f}'] {
            assert!(PostText::try_from(format!("before{control}after")).is_err());
        }
        assert!(PostText::try_from("before\u{007f}after".to_owned()).is_ok());
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BoardState {
    Open,
    Closed,
    Archiving,
    Archived,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceState {
    Active,
    Archiving,
    Archived,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BoardKind {
    Initiative { goal: String, done_when: String },
    Area {},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ThreadState {
    Open,
    Resolved,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workspace {
    pub envelope: crate::RecordEnvelope,
    pub workspace_id: WorkspaceId,
    pub name: ResourceName,
    pub description: Description,
    pub state: WorkspaceState,
    pub repositories: Vec<crate::RepositoryRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Board {
    pub envelope: crate::RecordEnvelope,
    pub board_id: BoardId,
    pub workspace_id: WorkspaceId,
    pub name: ResourceName,
    pub description: Description,
    pub kind: BoardKind,
    pub state: BoardState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectDirectoryEntry {
    pub envelope: crate::RecordEnvelope,
    pub id: ProjectId,
    pub board: BoardId,
    pub title: ResourceName,
}
