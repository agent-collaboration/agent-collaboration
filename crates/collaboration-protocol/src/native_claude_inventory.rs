//! Native jobs, peers and read completeness carry distinct facts and identities.
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NativeClaudeJobHandle(String);

impl TryFrom<String> for NativeClaudeJobHandle {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 128
            || value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(
                "native job handle must contain 1 to 128 bytes without whitespace or controls",
            );
        }
        Ok(Self(value))
    }
}
impl From<NativeClaudeJobHandle> for String {
    fn from(value: NativeClaudeJobHandle) -> Self {
        value.0
    }
}
impl JsonSchema for NativeClaudeJobHandle {
    fn schema_name() -> Cow<'static, str> {
        "NativeClaudeJobHandle".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"string","minLength":1,"maxLength":128,"x-maxUtf8Bytes":128})
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NativeClaudeStateName(String);
impl TryFrom<String> for NativeClaudeStateName {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
            return Err("native state name must contain 1 to 128 bytes without controls");
        }
        Ok(Self(value))
    }
}
impl From<NativeClaudeStateName> for String {
    fn from(value: NativeClaudeStateName) -> Self {
        value.0
    }
}
impl JsonSchema for NativeClaudeStateName {
    fn schema_name() -> Cow<'static, str> {
        "NativeClaudeStateName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"string","minLength":1,"maxLength":128,"x-maxUtf8Bytes":128})
    }
}

#[derive(JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NativeClaudeBackgroundState {
    Working,
    Blocked,
    Done,
    Failed,
    Stopped,
    Unknown { state: NativeClaudeStateName },
}
impl NativeClaudeBackgroundState {
    pub fn from_vendor_state(state: String) -> Result<Self, &'static str> {
        Ok(match state.as_str() {
            "working" => Self::Working,
            "blocked" => Self::Blocked,
            "done" => Self::Done,
            "failed" => Self::Failed,
            "stopped" => Self::Stopped,
            _ => Self::Unknown {
                state: state.try_into()?,
            },
        })
    }
    #[must_use]
    pub const fn is_loaded(&self) -> bool {
        matches!(self, Self::Working | Self::Blocked)
    }
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Working)
    }
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Stopped)
    }
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeInventoryUnavailableReason {
    ExecutableUnavailable,
    ReadTimedOut,
    ReadUnavailable,
}

impl std::fmt::Display for NativeInventoryUnavailableReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ExecutableUnavailable => "executable unavailable",
            Self::ReadTimedOut => "read timed out",
            Self::ReadUnavailable => "read unavailable",
        })
    }
}

#[derive(JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum NativeClaudeInventoryPart {
    NotRequested,
    Complete,
    Incomplete {
        unidentified_jobs: u32,
    },
    Unavailable {
        reason: NativeInventoryUnavailableReason,
    },
}

#[derive(JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NativeClaudeConflictReason {
    DuplicateLiveClaims { claims: u32 },
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeInventorySource {
    PeerRegistry,
    BackgroundJobs,
}

#[derive(JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeInventoryInvalidReason {
    InvalidOutput,
    UnexplainedSkippedRecords,
    DuplicateSessionIdentity,
}

impl std::fmt::Display for NativeInventorySource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::PeerRegistry => "peer registry",
            Self::BackgroundJobs => "background jobs",
        })
    }
}

impl std::fmt::Display for NativeInventoryInvalidReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidOutput => "invalid output",
            Self::UnexplainedSkippedRecords => "unexplained skipped records",
            Self::DuplicateSessionIdentity => "duplicate session identity",
        })
    }
}

impl NativeClaudeConflictReason {
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::DuplicateLiveClaims { .. } => "multiple live peers claim this session",
        }
    }
}
