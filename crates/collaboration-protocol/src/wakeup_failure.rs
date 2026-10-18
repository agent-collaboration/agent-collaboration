//! Local reminder failures preserve mutation uncertainty and an explicit next action.
use crate::LocalMutationEvidence;
use agent_automation::{OperationId, WakeupId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum WakeFailureReason {
    InvalidField {
        field: String,
        constraint: String,
    },
    OperationConflict,
    AutomationUnavailable,
    ResourceNotFound,
    InvalidRecord,
    LifecycleConflict,
    Overloaded,
    #[serde(untagged)]
    Refused(#[schemars(schema_with = "crate::WakeRefusal::json_schema")] crate::WakeRefusal),
}
#[derive(Clone, Copy, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WakeFailureStage {
    Validation,
    Admission,
    Storage,
    Inspection,
}
#[derive(Clone, Copy, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WakeNextAction {
    CorrectRequest,
    InspectOperation,
    VerifyResourceAddress,
    InspectWakeup,
    RetryLater,
}
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WakeFailure {
    #[serde(flatten)]
    pub reason: WakeFailureReason,
    pub stage: WakeFailureStage,
    pub message: String,
    pub operation_id: Option<OperationId>,
    pub wakeup_id: Option<WakeupId>,
    pub effects: LocalMutationEvidence,
    pub next_action: WakeNextAction,
}

// Serde flatten + deny_unknown_fields cannot decode a tagged payload reliably.
// Decode a closed wire object, then validate the kind-dependent fields explicitly.
impl<'de> Deserialize<'de> for WakeFailure {
    fn deserialize<TDeserializer: serde::Deserializer<'de>>(
        deserializer: TDeserializer,
    ) -> Result<Self, TDeserializer::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Envelope {
            stage: WakeFailureStage,
            message: String,
            #[serde(deserialize_with = "Option::deserialize")]
            operation_id: Option<OperationId>,
            #[serde(deserialize_with = "Option::deserialize")]
            wakeup_id: Option<WakeupId>,
            effects: LocalMutationEvidence,
            next_action: WakeNextAction,
        }
        let invalid = || serde::de::Error::custom("invalid wake failure kind or field guidance");
        let mut fields = serde_json::Map::<String, serde_json::Value>::deserialize(deserializer)?;
        let mut reason_fields = serde_json::Map::new();
        for name in [
            "kind",
            "field",
            "constraint",
            "wakeup",
            "creator",
            "max",
            "min",
        ] {
            if let Some(value) = fields.remove(name) {
                reason_fields.insert(name.into(), value);
            }
        }
        // Preserve presence, including null: irrelevant fields may not disappear.
        let kind = reason_fields
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(invalid)?;
        if matches!(
            kind,
            "operationConflict"
                | "automationUnavailable"
                | "resourceNotFound"
                | "invalidRecord"
                | "lifecycleConflict"
                | "overloaded"
        ) && reason_fields.len() != 1
        {
            return Err(invalid());
        }
        let reason: WakeFailureReason =
            serde_json::from_value(serde_json::Value::Object(reason_fields))
                .map_err(serde::de::Error::custom)?;
        if matches!(&reason, WakeFailureReason::InvalidField { field, constraint } if field.is_empty() || constraint.is_empty())
        {
            return Err(invalid());
        }
        let wire: Envelope = serde_json::from_value(serde_json::Value::Object(fields))
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            reason,
            stage: wire.stage,
            message: wire.message,
            operation_id: wire.operation_id,
            wakeup_id: wire.wakeup_id,
            effects: wire.effects,
            next_action: wire.next_action,
        })
    }
}
