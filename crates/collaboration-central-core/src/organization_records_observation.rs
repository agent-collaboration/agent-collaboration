//! Conformance only: read an organization's stamp and change records back
//! through the Fleet object that owns them, on any host.
use crate::organization_authority::{OrganizationChangeRecord, StoredStampRead};
use fleet_identity_protocol::OrganizationId;
use serde::{Deserialize, Serialize};

pub const ORGANIZATION_RECORDS_PATH: &str = "/admin/v1/conformance/organization-records";
/// Conformance only: overwrite the stored statement in place, as damage on disk would.
pub const ORGANIZATION_STATEMENT_TAMPER_PATH: &str =
    "/admin/v1/conformance/organization-statement-tamper";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrganizationStatementTamper {
    pub organization: OrganizationId,
    pub statement: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrganizationRecordsRequest {
    pub organization: OrganizationId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationRecordsObservation {
    /// The stored statement exactly as received, when the store is stamped.
    pub statement: Option<String>,
    pub stamped_organization: Option<OrganizationId>,
    pub records: Vec<OrganizationChangeRecord>,
}

impl OrganizationRecordsObservation {
    pub(crate) fn new(stamp: StoredStampRead, records: Vec<OrganizationChangeRecord>) -> Self {
        let (statement, stamped_organization) = match stamp {
            StoredStampRead::Unprovisioned => (None, None),
            StoredStampRead::Stamped(stamp) => (
                Some(stamp.statement().to_owned()),
                Some(stamp.organization_id().clone()),
            ),
        };
        Self {
            statement,
            stamped_organization,
            records,
        }
    }
}
