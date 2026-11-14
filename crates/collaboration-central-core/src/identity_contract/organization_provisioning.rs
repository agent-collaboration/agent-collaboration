//! The sign-in service provisions an organization it already created.
use fleet_identity_protocol::OrganizationId;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Only the private sign-in-to-central link exposes this operation.
pub const PROVISION_ORGANIZATION_ROUTE: &str = "/internal/organization-provisioning";
/// Renewal reads the stored statement's digest over the same private link.
pub const ORGANIZATION_STATEMENT_DIGEST_ROUTE: &str = "/internal/organization-statement-digest";

/// The signed stamp exactly as the sign-in service produced it: a compact JWS.
/// Its signature and claims are checked by the organization verifier, not by this type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OrganizationStatement(String);

/// Large enough for the closed stamp profile; a bound, not the profile check.
const MAXIMUM_STATEMENT_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("organization statement is not a bounded compact JWS")]
pub struct OrganizationStatementShapeError;

impl TryFrom<String> for OrganizationStatement {
    type Error = OrganizationStatementShapeError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let segments = value.split('.').collect::<Vec<_>>();
        let base64url = |segment: &&str| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        };
        if value.len() > MAXIMUM_STATEMENT_BYTES
            || segments.len() != 3
            || !segments.iter().all(base64url)
        {
            return Err(OrganizationStatementShapeError);
        }
        Ok(Self(value))
    }
}
impl From<OrganizationStatement> for String {
    fn from(value: OrganizationStatement) -> Self {
        value.0
    }
}
impl OrganizationStatement {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl JsonSchema for OrganizationStatement {
    fn schema_name() -> Cow<'static, str> {
        "OrganizationStatement".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 5,
            "maxLength": MAXIMUM_STATEMENT_BYTES,
            "pattern": "^[A-Za-z0-9_-]+\\.[A-Za-z0-9_-]+\\.[A-Za-z0-9_-]+$"
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvisionOrganization {
    pub organization_id: OrganizationId,
    pub organization_statement: OrganizationStatement,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProvisionOrganizationResult {
    Provisioned {},
}

/// Renewal names the organization whose stored statement it will replace.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadOrganizationStatementDigest {
    pub organization_id: OrganizationId,
}

/// What renewal needs about the stored statement; the statement never crosses.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrganizationStatementDigestReply {
    /// Lowercase hex SHA-256 of the stored statement's compact bytes.
    #[schemars(regex(pattern = r"^[0-9a-f]{64}$"))]
    pub digest: String,
    /// The verified statement's `exp`, in integer seconds.
    pub expires_at: i64,
    /// The verified statement's header `kid`.
    pub signing_key_id: String,
}
