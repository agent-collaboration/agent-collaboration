//! Object-call contracts between the Organization Registry and an authority.
use super::{
    CommandId, DecideDeviceLogin, DeviceLoginRefusal, DeviceLoginRequestId, VerifiedEmail,
};
use fleet_identity_protocol::{EnrollmentId, OrganizationId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorizeDeviceApproval {
    pub decision: DecideDeviceLogin,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ApprovalAuthorization {
    Reserved {},
    Refused { reason: DeviceLoginRefusal },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettleDeviceApproval {
    pub organization_id: OrganizationId,
    pub request_id: DeviceLoginRequestId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ApprovalActivation {
    Activated { enrollment_id: EnrollmentId },
    NotReserved {},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ApprovalCancellation {
    Cancelled {},
    NotReserved {},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitializeOrganization {
    pub organization_id: OrganizationId,
    pub owner_email: VerifiedEmail,
    pub command_id: CommandId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OrganizationInitialization {
    Initialized {},
    AlreadyInitialized {},
    NotThisCentral {},
}
