//! The sign-in service pushes its latest fence revisions; central keeps them
//! as floors and acknowledges the revision it retains after commit (r3 §5).
use fleet_identity_protocol::OrganizationId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Only the private sign-in-to-central link exposes this operation.
pub const AUTHORIZATION_REVISIONS_ROUTE: &str = "/internal/organization-authorization-revisions";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OrganizationFenceState {
    Active,
    Stopped,
    Deleted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PersonFenceState {
    Active,
    Inactive,
}

/// A fence revision: a positive JavaScript-safe integer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct FenceRevision(i64);
impl TryFrom<i64> for FenceRevision {
    type Error = FenceRevisionError;
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if !(1..=9_007_199_254_740_991).contains(&value) {
            return Err(FenceRevisionError);
        }
        Ok(Self(value))
    }
}
impl From<FenceRevision> for i64 {
    fn from(value: FenceRevision) -> Self {
        value.0
    }
}
impl FenceRevision {
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}
impl JsonSchema for FenceRevision {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FenceRevision".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"integer","minimum":1,"maximum":9_007_199_254_740_991_i64})
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("fence revision must be an integer in 1..=2^53-1")]
pub struct FenceRevisionError;

/// Lowercase hex SHA-256 of the sign-in service's canonical permission policy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PermissionPolicyDigest(String);
impl TryFrom<String> for PermissionPolicyDigest {
    type Error = FenceRevisionError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(FenceRevisionError);
        }
        Ok(Self(value))
    }
}
impl From<PermissionPolicyDigest> for String {
    fn from(value: PermissionPolicyDigest) -> Self {
        value.0
    }
}
impl PermissionPolicyDigest {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl JsonSchema for PermissionPolicyDigest {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PermissionPolicyDigest".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string","pattern":"^[0-9a-f]{64}$"})
    }
}

/// A sign-in person or subject identity (UUIDv7).
pub type FencePersonId = message_board::RecordId;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AuthorizationRevision {
    Organization {
        organization_id: OrganizationId,
        revision: FenceRevision,
        state: OrganizationFenceState,
        permission_policy_digest: PermissionPolicyDigest,
    },
    Person {
        organization_id: OrganizationId,
        person_id: FencePersonId,
        subject_id: FencePersonId,
        revision: FenceRevision,
        state: PersonFenceState,
    },
}

impl AuthorizationRevision {
    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        match self {
            Self::Organization {
                organization_id, ..
            }
            | Self::Person {
                organization_id, ..
            } => organization_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorizationRevisionAcknowledgement {
    /// The floor central retains after the commit; may exceed the sent revision.
    pub acknowledged_revision: FenceRevision,
}
