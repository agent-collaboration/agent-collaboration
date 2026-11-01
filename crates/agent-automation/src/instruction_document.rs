//! Reusable instruction text and its current materialized revision.
use crate::{InstructionId, RevisionId};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum InstructionTextError {
    #[error(
        "limitExceeded: INSTRUCTION_MAX_BYTES: {value}; allowed 1..={maximum}, step 1 UTF-8 bytes"
    )]
    LimitExceeded { value: usize, maximum: usize },
    #[error("instruction text must be NUL-free")]
    InvalidText,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct InstructionText(String);
impl TryFrom<String> for InstructionText {
    type Error = InstructionTextError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value.len() > Self::MAX_BYTES {
            return Err(InstructionTextError::LimitExceeded {
                value: value.len(),
                maximum: Self::MAX_BYTES,
            });
        }
        if value.contains('\0') {
            return Err(InstructionTextError::InvalidText);
        }
        Ok(Self(value))
    }
}
impl From<InstructionText> for String {
    fn from(value: InstructionText) -> Self {
        value.0
    }
}
impl InstructionText {
    pub const MAX_BYTES: usize = 1_048_576;
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstructionDocument {
    pub instruction_id: InstructionId,
    pub revision_id: RevisionId,
    pub text: InstructionText,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl schemars::JsonSchema for InstructionText {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "InstructionText".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string","minLength":1,"maxLength":Self::MAX_BYTES,"pattern":"^[^\\u0000]+$","x-maxUtf8Bytes":Self::MAX_BYTES,
            "description":format!("INSTRUCTION_MAX_BYTES: 1..={} UTF-8 bytes; nonempty and NUL-free. maxLength is a character pre-check; entry enforces the byte bound.", Self::MAX_BYTES)})
    }
}
