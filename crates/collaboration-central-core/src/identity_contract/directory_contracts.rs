//! Public discovery and the directory's private requests to a central.
use super::{
    CommandId, DecideDeviceLogin, DeviceLoginRefusal, DeviceLoginRequestId, HttpsUrl, ResourceUri,
    VerifiedEmail,
};
use fleet_identity_protocol::{EnrollmentId, OrganizationId};
use fleet_sync_protocol::{PeerId, ReceiverId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FindCentral {
    pub organization_id: OrganizationId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CentralLookup {
    Found { central: CentralAddresses },
    Creating {},
    UnknownOrganization {},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CentralAddresses {
    pub session_endpoint: HttpsUrl,
    pub machine_address: ResourceUri,
    pub admin_address: ResourceUri,
    pub admission_address: ResourceUri,
    pub standing_address: ResourceUri,
    pub machine_token_address: ResourceUri,
    #[schemars(with = "message_board::RecordId")]
    pub receiver: ReceiverId,
    #[schemars(with = "message_board::RecordId")]
    pub relay: PeerId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CentralRecord {
    pub addresses: CentralAddresses,
    pub directory_calls: DirectoryCallAddresses,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryCallAddresses {
    pub approval_authorize_address: ResourceUri,
    pub approval_activate_address: ResourceUri,
    pub approval_cancel_address: ResourceUri,
    pub initialization_address: ResourceUri,
}

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
