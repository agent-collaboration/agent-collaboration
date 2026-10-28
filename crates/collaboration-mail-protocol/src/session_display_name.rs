//! One validated sender label is shared by frozen mail and native push headers.
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SessionDisplayName(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("session display name must be 1 to 120 characters, contain no controls or header arrows")]
pub struct SessionDisplayNameError;

const MAX_SESSION_DISPLAY_NAME_CHARS: usize = 120;

impl TryFrom<String> for SessionDisplayName {
    type Error = SessionDisplayNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty()
            || value.chars().count() > MAX_SESSION_DISPLAY_NAME_CHARS
            || value.chars().any(char::is_control)
            || value.contains(" ← ")
            || value.contains(" → ")
        {
            return Err(SessionDisplayNameError);
        }
        Ok(Self(value))
    }
}

impl SessionDisplayName {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for SessionDisplayName {
    fn schema_name() -> Cow<'static, str> {
        "SessionDisplayName".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type":"string",
            "minLength":1,
            "maxLength":120,
            "pattern":"^[^\\u0000-\\u001F]+$"
        })
    }
}

impl From<SessionDisplayName> for String {
    fn from(value: SessionDisplayName) -> Self {
        value.0
    }
}
