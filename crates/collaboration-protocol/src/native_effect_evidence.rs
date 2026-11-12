//! Native effect evidence: what a session hand-off allocated, resumed, submitted and stopped.
use crate::{CodexGeneration, SessionRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeEffectEvidence {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_differences: Vec<crate::PermissionDifference>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub target: Option<SessionRef>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub generation: Option<CodexGeneration>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub client_user_message_id: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub native_turn_id: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub native_submission_id: Option<String>,
    pub allocation: PreparationEffect,
    pub resume: PreparationEffect,
    pub submission: SubmissionEffect,
    pub cessation: CessationEvidence,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreparationEffect {
    NotRequested,
    NotDispatched,
    Accepted,
    Rejected,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubmissionEffect {
    NotDispatched,
    Dispatching,
    Accepted,
    Rejected,
    Unknown,
}
#[derive(Clone, Copy, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CessationEvidence {
    NotApplicable,
    Unconfirmed,
    Confirmed,
}
