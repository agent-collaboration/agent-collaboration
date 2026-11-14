//! Values admitted by the issuer's device-login contract.
use message_board::WorkspaceId;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub const MAX_ALLOWED_WORKSPACES: usize = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum EnrollPermission {
    #[serde(rename = "machine:enroll")]
    MachineEnroll,
}

#[derive(Debug, thiserror::Error, Clone, Eq, PartialEq)]
#[error("allowedWorkspaces holds at most 100 workspaces")]
pub struct TooManyAllowedWorkspaces;

/// The original approval's ordered workspace snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<WorkspaceId>", into = "Vec<WorkspaceId>")]
pub struct AllowedWorkspaces(Vec<WorkspaceId>);

impl TryFrom<Vec<WorkspaceId>> for AllowedWorkspaces {
    type Error = TooManyAllowedWorkspaces;
    fn try_from(value: Vec<WorkspaceId>) -> Result<Self, Self::Error> {
        if value.len() > MAX_ALLOWED_WORKSPACES {
            return Err(TooManyAllowedWorkspaces);
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
