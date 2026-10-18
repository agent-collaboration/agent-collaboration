//! A thread references one subject; its display title is resolved from current records.
use crate::{
    AdrId, RecordEnvelope, TaskId, TextValidationError, ThreadId, WorkTrailRowId, WorkstreamId,
};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SubjectLine(String);
impl TryFrom<String> for SubjectLine {
    type Error = TextValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let value = value.trim();
        if !(1..=120).contains(&value.chars().count()) || value.contains(['\r', '\n']) {
            return Err(TextValidationError {
                field: "about",
                requirement: "must be one nonempty line of at most 120 characters",
            });
        }
        Ok(Self(value.to_owned()))
    }
}
impl From<SubjectLine> for String {
    fn from(value: SubjectLine) -> Self {
        value.0
    }
}
impl SubjectLine {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl JsonSchema for SubjectLine {
    fn schema_name() -> Cow<'static, str> {
        "SubjectLine".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"string","minLength":1,"maxLength":120,"pattern":"^[^\\r\\n]+$"})
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "subject",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ThreadSubject {
    Project { line: SubjectLine },
    Workstream(WorkstreamId),
    Task(TaskId),
    WorkTrailRow(WorkTrailRowId),
    WorkspaceAdr(AdrId),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "title",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum SubjectTitle {
    Shown(String),
    Redacted,
    Unavailable,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadSubjectView {
    pub about: ThreadSubject,
    pub title: SubjectTitle,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadRefiled {
    pub envelope: RecordEnvelope,
    pub thread: ThreadId,
    pub from: ThreadSubject,
    pub to: ThreadSubject,
}
