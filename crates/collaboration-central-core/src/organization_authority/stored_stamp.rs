use super::OrganizationStatementDigest;
use fleet_identity_protocol::OrganizationId;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredOrganizationStamp {
    organization: OrganizationId,
    statement: String,
}
impl StoredOrganizationStamp {
    pub(crate) fn new(organization: OrganizationId, statement: String) -> Self {
        Self {
            organization,
            statement,
        }
    }
    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        &self.organization
    }
    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }
    #[must_use]
    pub fn digest(&self) -> OrganizationStatementDigest {
        OrganizationStatementDigest::of_compact_jws(&self.statement)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoredStampRead {
    Unprovisioned,
    Stamped(StoredOrganizationStamp),
}
