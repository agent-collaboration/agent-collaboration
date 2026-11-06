//! Closed remote-create contracts shared by Fleet admission and Router consumers.
use crate::MailValidationError;
use fleet_sync_protocol::PeerId;
use message_board::{EndpointId, RequestId, SessionRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use fleet_identity_protocol::{
    AbsolutePath, ModelName as ModelId, RemoteLaunchAccess as AccessMode,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteCreateRequest {
    pub request_id: RequestId,
    pub requester: SessionRef,
    pub target_machine: PeerId,
    pub command: CreateSessionCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSessionCommand {
    pub endpoint: EndpointId,
    pub working_directory: AbsolutePath,
    pub access: AccessMode,
    pub model: Option<ModelId>,
    pub effort: Option<Effort>,
    pub mode: Option<SessionMode>,
    pub first_input: Option<FirstInput>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteCreateAdmission {
    Accepted {
        request_id: RequestId,
    },
    Replayed {
        request_id: RequestId,
        status: RemoteCreateStatus,
    },
    Refused {
        reason: RemoteCreateRefusal,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RemoteCreateRefusal {
    #[serde(deserialize_with = "deserialize_closed_unit")]
    SenderNotEnrolled,
    #[serde(deserialize_with = "deserialize_closed_unit")]
    TargetNotEnrolled,
    /// Temporary offline refusal until remote start is redesigned (R18).
    #[serde(deserialize_with = "deserialize_closed_unit")]
    TargetOffline,
    #[serde(deserialize_with = "deserialize_closed_unit")]
    Conflict,
    #[serde(deserialize_with = "deserialize_closed_unit")]
    TooOld,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CreateSessionResult {
    Created {
        session: SessionRef,
        settings: SettingsOutcome,
        first_input: FirstInputOutcome,
    },
    Refused {
        reason: CreateRefusal,
    },
    #[serde(deserialize_with = "deserialize_closed_unit")]
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SettingsOutcome {
    #[serde(deserialize_with = "deserialize_closed_unit")]
    AllApplied,
    Partial {
        #[serde(
            deserialize_with = "decode_setting_names",
            serialize_with = "encode_setting_names"
        )]
        #[schemars(length(max = 4))]
        not_applied: Vec<SettingName>,
    },
}

impl SettingsOutcome {
    pub fn partial(not_applied: Vec<SettingName>) -> Result<Self, MailValidationError> {
        validate_setting_names(&not_applied)?;
        Ok(Self::Partial { not_applied })
    }
}

fn validate_setting_names(names: &[SettingName]) -> Result<(), MailValidationError> {
    // The PD's 64 KiB acceptance budget includes at most the four setting names.
    (names.len() <= 4).then_some(()).ok_or(MailValidationError {
        field: "notApplied",
    })
}

fn decode_setting_names<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Vec<SettingName>, D::Error> {
    let names = Vec::<SettingName>::deserialize(decoder)?;
    validate_setting_names(&names).map_err(serde::de::Error::custom)?;
    Ok(names)
}

fn encode_setting_names<S: serde::Serializer>(
    names: &[SettingName],
    encoder: S,
) -> Result<S::Ok, S::Error> {
    validate_setting_names(names).map_err(serde::ser::Error::custom)?;
    names.serialize(encoder)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FirstInputOutcome {
    #[serde(deserialize_with = "deserialize_closed_unit")]
    NotRequested,
    #[serde(deserialize_with = "deserialize_closed_unit")]
    NotAttempted,
    #[serde(deserialize_with = "deserialize_closed_unit")]
    Accepted,
    Failed {
        reason: PromptFailure,
    },
    #[serde(deserialize_with = "deserialize_closed_unit")]
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RemoteCreateStatus {
    #[serde(deserialize_with = "deserialize_closed_unit")]
    Admitted,
    #[serde(deserialize_with = "deserialize_closed_unit")]
    AtTarget,
    Completed {
        result: CreateSessionResult,
    },
    #[serde(deserialize_with = "deserialize_closed_unit")]
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteCreateOutcome {
    Completed(CreateSessionResult),
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CreateRefusal {
    #[serde(deserialize_with = "deserialize_closed_unit")]
    NoRemoteApprover,
    #[serde(deserialize_with = "deserialize_closed_unit")]
    EndpointUnavailable,
    #[serde(deserialize_with = "deserialize_closed_unit")]
    InvalidWorkingDirectory,
    #[serde(deserialize_with = "deserialize_closed_unit")]
    InvalidSettings,
    ProviderRefused {
        code: BoundedCode,
        detail: BoundedText,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum PromptFailure {
    Refused {
        code: BoundedCode,
        detail: BoundedText,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum SettingName {
    Model,
    Effort,
    Mode,
    Access,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSessionBody {
    pub request_id: RequestId,
    pub requester: SessionRef,
    pub command: CreateSessionCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteCreateResultBody {
    pub request_id: RequestId,
    pub outcome: RemoteCreateOutcome,
}

macro_rules! validated_create_string {
    ($name:ident, $field:literal, $validate:expr, $schema:expr) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, MailValidationError> {
                let value = value.into();
                if value.is_empty() || !($validate)(&value) {
                    return Err(MailValidationError { field: $field });
                }
                Ok(Self(value))
            }
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = MailValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                $schema
            }
        }
    };
}
validated_create_string!(
    BoundedCode,
    "code",
    |value: &str| value.len() <= 64
        && value.bytes().all(|byte| byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'_' | b'.' | b'-')),
    schemars::json_schema!({"type":"string","minLength":1,"maxLength":64,"pattern":"^[a-z0-9_.-]+$","x-maxUtf8Bytes":64})
);
validated_create_string!(
    BoundedText,
    "detail",
    |value: &str| value.len() <= 1024,
    schemars::json_schema!({"type":"string","minLength":1,"maxLength":1024,"x-maxUtf8Bytes":1024})
);
validated_create_string!(
    Effort,
    "effort",
    |value: &str| !value.contains('\0'),
    schemars::json_schema!({"type":"string","minLength":1,"pattern":"^[^\\u0000]+$"})
);
validated_create_string!(
    SessionMode,
    "mode",
    |value: &str| !value.contains('\0'),
    schemars::json_schema!({"type":"string","minLength":1,"pattern":"^[^\\u0000]+$"})
);
// Match the native prompt's 1 MiB text contract without an upward dependency
// on collaboration-protocol or a module cycle through remote-wake sender rules.
validated_create_string!(
    FirstInput,
    "firstInput",
    |value: &str| value.len() <= 1024 * 1024
        && !value
            .chars()
            .any(|character| character <= '\u{001f}' && !matches!(character, '\n' | '\t')),
    schemars::json_schema!({"type":"string","minLength":1,"maxLength":1048576,"x-maxUtf8Bytes":1048576,"pattern":"^[^\\u0000-\\u0008\\u000B\\u000C\\u000E-\\u001F]+$"})
);

// Serde ignores extra map fields for internally tagged unit variants. Decode
// their payload as a closed empty struct while keeping the public unit variants.
pub(crate) fn deserialize_closed_unit<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<(), D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    Empty::deserialize(decoder).map(|_| ())
}

#[cfg(test)]
#[path = "remote_create_test.rs"]
mod tests;
