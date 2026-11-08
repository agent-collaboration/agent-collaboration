//! Bounded values of the device-login decision, matching the issuer's
//! `decideDeviceLoginSchema` field for field.
use message_board::{IdentityValidationError, MachineLabel, ServiceId, WorkspaceId};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// The issuer counts a label's length in UTF-16 code units, as JavaScript does.
pub const MAX_DEVICE_LOGIN_LABEL_UTF16_UNITS: usize = 120;
pub const MAX_ALLOWED_WORKSPACES: usize = 100;

/// The permission the issuer checked before asking: always `machine:enroll`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum EnrollPermission {
    #[serde(rename = "machine:enroll")]
    MachineEnroll,
}

/// A machine label as the approver typed it: trimmed, 1 to 120 UTF-16 code
/// units, and a valid board machine label.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DeviceLoginLabel(MachineLabel);

#[derive(Debug, thiserror::Error, Clone, Eq, PartialEq)]
pub enum DeviceLoginValueError {
    #[error("label must be a trimmed machine label of 1 to 120 UTF-16 code units")]
    Label,
    #[error("allowedWorkspaces holds at most 100 workspaces")]
    TooManyWorkspaces,
    #[error(transparent)]
    FormerService(#[from] IdentityValidationError),
    #[error("formerServiceId must be a UUIDv4 or UUIDv7")]
    FormerServiceVersion,
}

impl TryFrom<String> for DeviceLoginLabel {
    type Error = DeviceLoginValueError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let label = MachineLabel::try_from(value).map_err(|_| DeviceLoginValueError::Label)?;
        if label.as_str().encode_utf16().count() > MAX_DEVICE_LOGIN_LABEL_UTF16_UNITS {
            return Err(DeviceLoginValueError::Label);
        }
        Ok(Self(label))
    }
}
impl From<DeviceLoginLabel> for String {
    fn from(value: DeviceLoginLabel) -> Self {
        value.0.into()
    }
}
impl DeviceLoginLabel {
    #[must_use]
    pub fn as_machine_label(&self) -> &MachineLabel {
        &self.0
    }
}
impl JsonSchema for DeviceLoginLabel {
    fn schema_name() -> Cow<'static, str> {
        "DeviceLoginLabel".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = MachineLabel::json_schema(generator);
        schema.insert(
            "maxLength".into(),
            MAX_DEVICE_LOGIN_LABEL_UTF16_UNITS.into(),
        );
        schema.insert(
            "x-maxUtf16Units".into(),
            MAX_DEVICE_LOGIN_LABEL_UTF16_UNITS.into(),
        );
        schema
    }
}

/// The workspaces the approver allowed this machine into, at most 100.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<WorkspaceId>", into = "Vec<WorkspaceId>")]
pub struct AllowedWorkspaces(Vec<WorkspaceId>);

impl TryFrom<Vec<WorkspaceId>> for AllowedWorkspaces {
    type Error = DeviceLoginValueError;
    fn try_from(value: Vec<WorkspaceId>) -> Result<Self, Self::Error> {
        if value.len() > MAX_ALLOWED_WORKSPACES {
            return Err(DeviceLoginValueError::TooManyWorkspaces);
        }
        Ok(Self(value))
    }
}
impl From<AllowedWorkspaces> for Vec<WorkspaceId> {
    fn from(value: AllowedWorkspaces) -> Self {
        value.0
    }
}
impl AllowedWorkspaces {
    #[must_use]
    pub fn as_slice(&self) -> &[WorkspaceId] {
        &self.0
    }
}
impl JsonSchema for AllowedWorkspaces {
    fn schema_name() -> Cow<'static, str> {
        "AllowedWorkspaces".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let items = generator.subschema_for::<WorkspaceId>();
        json_schema!({"type":"array","items":items,"maxItems":MAX_ALLOWED_WORKSPACES})
    }
}

/// The service id this machine had before it moved to UUIDv7 identities.
/// Older installations used UUIDv4; current ones use UUIDv7.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FormerServiceId(ServiceId);

impl TryFrom<String> for FormerServiceId {
    type Error = DeviceLoginValueError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        // ServiceId is already a canonical lowercase hyphenated UUID, so the
        // version is the character at 14 and the variant the one at 19.
        let service = ServiceId::try_from(value)?;
        let characters = service.as_str().as_bytes();
        let version = characters.get(14).copied();
        let variant = characters.get(19).copied();
        if !matches!(version, Some(b'4' | b'7'))
            || !matches!(variant, Some(b'8' | b'9' | b'a' | b'b'))
        {
            return Err(DeviceLoginValueError::FormerServiceVersion);
        }
        Ok(Self(service))
    }
}
impl From<FormerServiceId> for String {
    fn from(value: FormerServiceId) -> Self {
        value.0.into()
    }
}
impl FormerServiceId {
    #[must_use]
    pub fn as_service_id(&self) -> &ServiceId {
        &self.0
    }
}
impl JsonSchema for FormerServiceId {
    fn schema_name() -> Cow<'static, str> {
        "FormerServiceId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type":"string","minLength":36,"maxLength":36,
            "pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[47][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$(?![\\s\\S])"})
    }
}
