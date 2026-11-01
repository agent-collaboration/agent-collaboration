//! Exact frozen bytes are the handover authority; the typed view is immutable.
use crate::{
    AutomationId, AutomationImportDefinition, AutomationImportHeader, AutomationKind,
    AutomationTiming, ChangeId, FiringId, GridTime, ImportId, InstructionText, RevisionId, RunId,
    SuspendedAutomationConfiguration, TimingRequest, WakeCreator,
};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationImportContent {
    pub revision_id: RevisionId,
    pub text: InstructionText,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CarriedAutomationFiring {
    pub firing_id: FiringId,
    pub due_ms: GridTime,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("allOf" = [{"required":["watermarkLocalMs","firstDueMs","admittedThroughMs","externalOccupant","carriedWaitingDueMs","carriedFiring"]}]))]
#[schemars(transform = import_payload_message_schema)]
pub struct AutomationImportPayload {
    pub automation_id: AutomationId,
    pub revision: ChangeId,
    pub creator: WakeCreator,
    pub header: AutomationImportHeader,
    pub definition: AutomationImportDefinition,
    pub content: AutomationImportContent,
    #[serde(deserialize_with = "Option::deserialize")]
    pub watermark_local_ms: Option<i64>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub first_due_ms: Option<GridTime>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub admitted_through_ms: Option<GridTime>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub external_occupant: Option<RunId>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub carried_waiting_due_ms: Option<GridTime>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub carried_firing: Option<CarriedAutomationFiring>,
}

#[derive(Debug, thiserror::Error)]
pub enum AutomationImportPayloadError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error(transparent)]
    Limit(#[from] crate::limits::LimitExceeded),
    #[error(transparent)]
    Timing(#[from] crate::AutomationValidationError),
    #[error("invalid frozen handover field: {0}")]
    Invalid(&'static str),
}

/// Bytes own the value; the private typed projection is decoded once and cannot diverge.
#[derive(Clone, Debug)]
pub struct FrozenAutomationImportPayload {
    bytes: String,
    decoded: AutomationImportPayload,
}

impl FrozenAutomationImportPayload {
    pub fn freeze(payload: AutomationImportPayload) -> Result<Self, AutomationImportPayloadError> {
        let bytes = serde_json_canonicalizer::to_vec(&payload)?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AutomationImportPayloadError> {
        let bytes = String::from_utf8(bytes.to_vec())?;
        let raw: Value = serde_json::from_str(&bytes)?;
        validate_raw_limits(&raw)?;
        if raw
            .pointer("/definition/value/destination/kind")
            .and_then(Value::as_str)
            == Some("freshEachRun")
            && raw.pointer("/definition/value/model").is_none()
        {
            return Err(AutomationImportPayloadError::Invalid("definition.model"));
        }
        let decoded: AutomationImportPayload = serde_json::from_value(raw.clone())?;
        require_retained_fields(&raw, &serde_json::to_value(&decoded)?)?;
        decoded.validate()?;
        Ok(Self { bytes, decoded })
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.as_bytes()
    }

    #[must_use]
    pub fn payload(&self) -> &AutomationImportPayload {
        &self.decoded
    }
}

impl Serialize for FrozenAutomationImportPayload {
    fn serialize<Serializer: serde::Serializer>(
        &self,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error> {
        serializer.serialize_str(&self.bytes)
    }
}

impl<'de> Deserialize<'de> for FrozenAutomationImportPayload {
    fn deserialize<Deserializer: serde::Deserializer<'de>>(
        deserializer: Deserializer,
    ) -> Result<Self, Deserializer::Error> {
        Self::from_bytes(String::deserialize(deserializer)?.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for FrozenAutomationImportPayload {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FrozenAutomationImportPayload".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"string","contentMediaType":"application/json",
            "contentSchema":generator.subschema_for::<AutomationImportPayload>(),
            "description":"The exact frozen JSON payload bytes, encoded as a JSON string; never re-encode between retries."})
    }
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationImportRequest {
    pub import_id: ImportId,
    pub payload: FrozenAutomationImportPayload,
}

#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationImportIdentity {
    pub automation_id: AutomationId,
}

impl AutomationImportPayload {
    fn validate(&self) -> Result<(), AutomationImportPayloadError> {
        use AutomationImportPayloadError::Invalid;
        if let Some(crate::ImportedContinuity::ImportedSummary {
            text,
            source_run_id,
            ..
        }) = &self.header.imported_continuity
        {
            crate::InstructionText::try_from(text.clone())
                .map_err(|_| Invalid("header.importedContinuity.text"))?;
            crate::NonEmptyText::try_from(source_run_id.clone())
                .map_err(|_| Invalid("header.importedContinuity.sourceRunId"))?;
        }
        for timestamp in [
            &self.header.anchor_at,
            &self.header.created_at,
            &self.header.updated_at,
        ] {
            require_millisecond_timestamp(timestamp)?;
        }
        for due in [
            self.first_due_ms,
            self.admitted_through_ms,
            self.carried_waiting_due_ms,
            self.carried_firing.as_ref().map(|firing| firing.due_ms),
        ]
        .into_iter()
        .flatten()
        {
            if chrono::DateTime::<chrono::Utc>::from_timestamp_millis(due.unix_millis()).is_none() {
                return Err(Invalid("grid timestamp out of range"));
            }
        }
        let timing = match &self.definition {
            AutomationImportDefinition::Schedule(definition) => Some(&definition.timing),
            AutomationImportDefinition::Wake(definition) => Some(&definition.timing),
            AutomationImportDefinition::Suspended(definition) => {
                if !matches!(definition.legacy_timing, TimingRequest::Cron { .. }) {
                    return Err(Invalid("legacyTiming"));
                }
                None
            }
        };
        match (timing, self.first_due_ms) {
            (Some(request), Some(first)) => {
                let before = first
                    .unix_millis()
                    .checked_sub(1)
                    .ok_or(Invalid("firstDueMs"))?;
                let resolved = AutomationTiming::resolve(request, self.definition.kind(), before)?;
                if resolved.first_due() != first {
                    return Err(Invalid("firstDueMs"));
                }
            }
            (None, None) => {}
            _ => return Err(Invalid("firstDueMs")),
        }
        if let Some(watermark) = self.watermark_local_ms
            && chrono::DateTime::<chrono::Utc>::from_timestamp_millis(watermark).is_none()
        {
            return Err(Invalid("watermarkLocalMs"));
        }
        match self.definition.kind() {
            AutomationKind::Schedule if self.carried_firing.is_some() => {
                return Err(Invalid("carriedFiring"));
            }
            AutomationKind::Wake => {
                if self.external_occupant.is_some() || self.carried_waiting_due_ms.is_some() {
                    return Err(Invalid("externalOccupant or carriedWaitingDueMs"));
                }
                if self.header.imported_continuity.is_some() {
                    return Err(Invalid("header.importedContinuity"));
                }
                crate::MessageText::try_from(self.content.text.as_str().to_owned())
                    .map_err(|_| Invalid("content.text"))?;
                let configuration = match &self.definition {
                    AutomationImportDefinition::Wake(definition) => &definition.configuration,
                    AutomationImportDefinition::Suspended(definition) => {
                        match &definition.configuration {
                            SuspendedAutomationConfiguration::Wake(configuration) => configuration,
                            SuspendedAutomationConfiguration::Schedule(_) => {
                                return Err(Invalid("definition.kind"));
                            }
                        }
                    }
                    AutomationImportDefinition::Schedule(_) => {
                        return Err(Invalid("definition.kind"));
                    }
                };
                require_millisecond_timestamp(&configuration.expires_at)?;
                if !matches!(
                    configuration.state,
                    crate::WakeState::Active | crate::WakeState::Paused | crate::WakeState::Expired
                ) {
                    return Err(Invalid("definition.configuration.state"));
                }
            }
            AutomationKind::Schedule => {}
        }
        Ok(())
    }
}

fn require_millisecond_timestamp(
    value: &crate::ObservationTimestamp,
) -> Result<(), AutomationImportPayloadError> {
    let timestamp: chrono::DateTime<chrono::Utc> = String::from(value.clone())
        .parse()
        .map_err(|_| AutomationImportPayloadError::Invalid("timestamp"))?;
    if !timestamp.timestamp_subsec_nanos().is_multiple_of(1_000_000) {
        return Err(AutomationImportPayloadError::Invalid("timestamp precision"));
    }
    Ok(())
}

fn validate_raw_limits(raw: &Value) -> Result<(), AutomationImportPayloadError> {
    use crate::limits::LimitName;
    let content_limit = if raw.pointer("/definition/kind").and_then(Value::as_str) == Some("wake")
        || raw
            .pointer("/definition/value/configuration/kind")
            .and_then(Value::as_str)
            == Some("wake")
    {
        LimitName::WakeMessageMaxBytes
    } else {
        LimitName::InstructionMaxBytes
    };
    for (pointer, limit) in [
        ("/content/text", content_limit),
        (
            "/header/importedContinuity/text",
            LimitName::InstructionMaxBytes,
        ),
        (
            "/header/importedContinuity/sourceRunId",
            LimitName::TextMaxBytes,
        ),
    ] {
        if let Some(text) = raw.pointer(pointer).and_then(Value::as_str) {
            limit.validate(text.len() as u64)?;
        }
    }
    match raw {
        Value::Object(fields) => {
            for (name, value) in fields {
                if matches!(name.as_str(), "generation" | "seq" | "reportSeq")
                    && let Some(counter) = value.as_u64()
                {
                    LimitName::JsonSafeInteger.validate(counter)?;
                }
                let limit = match name.as_str() {
                    "endpointId" => Some(LimitName::EndpointIdMaxBytes),
                    "sessionId" => Some(LimitName::SessionIdMaxBytes),
                    _ => None,
                };
                if let (Some(limit), Some(text)) = (limit, value.as_str()) {
                    limit.validate(text.len() as u64)?;
                }
                validate_raw_limits(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_raw_limits(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn require_retained_fields(raw: &Value, typed: &Value) -> Result<(), AutomationImportPayloadError> {
    match (raw, typed) {
        (Value::Object(fields), Value::Object(known)) => {
            for (name, value) in fields {
                let field = known
                    .get(name)
                    .ok_or(AutomationImportPayloadError::Invalid(
                        "unknown payload field",
                    ))?;
                require_retained_fields(value, field)?;
            }
        }
        (Value::Array(values), Value::Array(known)) => {
            if values.len() != known.len() {
                return Err(AutomationImportPayloadError::Invalid(
                    "dropped payload element",
                ));
            }
            for (value, field) in values.iter().zip(known) {
                require_retained_fields(value, field)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn import_payload_message_schema(schema: &mut Schema) {
    let message = crate::MessageText::json_schema(&mut SchemaGenerator::default());
    let rule = serde_json::json!({
        "if":{"anyOf":[
            {"properties":{"definition":{"properties":{"kind":{"const":"wake"}}}}},
            {"properties":{"definition":{"properties":{"kind":{"const":"suspended"},
                "value":{"properties":{"configuration":{"properties":{"kind":{"const":"wake"}}}}}}}}}
        ]},
        "then":{"properties":{"content":{"properties":{"text":message}}}}
    });
    if let Some(rules) = schema
        .as_object_mut()
        .and_then(|root| root.get_mut("allOf"))
        .and_then(Value::as_array_mut)
    {
        rules.push(rule);
    }
}
