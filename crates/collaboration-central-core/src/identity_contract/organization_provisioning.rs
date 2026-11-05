//! The sign-in service provisions an organization it already created.
use fleet_identity_protocol::OrganizationId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Only the private sign-in-to-central link exposes this operation.
pub const PROVISION_ORGANIZATION_ROUTE: &str = "/internal/organization-provisioning";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvisionOrganization {
    pub organization_id: OrganizationId,
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
