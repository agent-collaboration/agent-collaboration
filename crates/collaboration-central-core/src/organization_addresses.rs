//! Logical call identities carry their owning organization explicitly.
use fleet_identity_protocol::OrganizationId;
use fleet_sync_protocol::DatabaseId;
use message_board::ServiceId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FleetAddress {
    pub organization: OrganizationId,
}

impl FleetAddress {
    #[must_use]
    pub fn new(organization: OrganizationId) -> Self {
        Self { organization }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionAddress {
    pub organization: OrganizationId,
    pub installation: ServiceId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyAddress {
    pub organization: OrganizationId,
    pub database: DatabaseId,
}
