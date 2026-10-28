//! A leave has at most one follow-up, and resolution preserves its declared reason.
use crate::{Identity, TextValidationError};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ThreadResolutionReason(String);

impl TryFrom<String> for ThreadResolutionReason {
    type Error = TextValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err(TextValidationError {
                field: "reason",
                requirement: "must contain non-whitespace text",
            });
        }
        Ok(Self(value))
    }
}
impl From<ThreadResolutionReason> for String {
    fn from(reason: ThreadResolutionReason) -> Self {
        reason.0
    }
}
impl ThreadResolutionReason {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl JsonSchema for ThreadResolutionReason {
    fn schema_name() -> Cow<'static, str> {
        "ThreadResolutionReason".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"string","minLength":1,"pattern":"\\S"})
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum LeaveFollowUp {
    HandTo { to: Identity },
    Resolve { reason: ThreadResolutionReason },
}
