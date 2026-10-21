//! Bounded native failure facts retained with a rejected delivery receipt.
use crate::{CodexGeneration, PreparationEffect, SubmissionEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeDeliveryFailure {
    pub operation: NativeDeliveryOperation,
    pub stage: NativeDeliveryStage,
    pub cause: NativeDeliveryFailureCause,
    pub generation: CodexGeneration,
    pub resume: PreparationEffect,
    pub submission: SubmissionEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
pub enum NativeDeliveryOperation {
    #[serde(rename = "nativeAdmission")]
    NativeAdmission,
    #[serde(rename = "thread/read")]
    ThreadRead,
    #[serde(rename = "thread/resume")]
    ThreadResume,
    #[serde(rename = "thread/turns/list")]
    ThreadTurnsList,
    #[serde(rename = "turn/start")]
    TurnStart,
    #[serde(rename = "turn/steer")]
    TurnSteer,
    #[serde(rename = "thread/queue/add")]
    ThreadQueueAdd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeDeliveryStage {
    Inspect,
    Resume,
    Start,
    Steer,
    Queue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeDeliveryFailureCause {
    InvalidThreadResponse,
    RecordedResumePreparationFailed,
    InvalidResumeResponse,
    NativeRejected,
    BackendEndpointMismatch,
    NativeSchemaUnavailable,
    NativeSchemaMismatch,
    NativeMethodUnavailable,
    InvalidNativeRequest,
    NativeRequestUnavailable,
    MissingActiveTurn,
}

impl NativeDeliveryFailureCause {
    #[must_use]
    pub const fn human_phrase(self) -> &'static str {
        match self {
            Self::InvalidThreadResponse => "Thread response failed validation",
            Self::RecordedResumePreparationFailed => "Recorded resume preparation failed",
            Self::InvalidResumeResponse => "Recorded resume response failed validation",
            Self::NativeRejected => "Native operation was rejected",
            Self::BackendEndpointMismatch => "Native backend does not match the target endpoint",
            Self::NativeSchemaUnavailable => "Native schema is unavailable",
            Self::NativeSchemaMismatch => {
                "Advertised native schema or generation does not match admission"
            }
            Self::NativeMethodUnavailable => "Native method is unsupported",
            Self::InvalidNativeRequest => "Native request failed local validation",
            Self::NativeRequestUnavailable => "Native request was unavailable before dispatch",
            Self::MissingActiveTurn => "Active turn could not be identified",
        }
    }
}
