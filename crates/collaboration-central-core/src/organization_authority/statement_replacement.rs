//! Pure replacement decisions take the stamp read by the applying transaction.
use super::{
    OrganizationStatementDigest, StoredOrganizationStamp, VerifiedFleetOrganizationStatement,
};
use fleet_identity_protocol::OrganizationId;

/// What the applying transaction read: the stamp's organization and the digest
/// of its compact bytes. The stored statement's signature was checked when it
/// arrived and again whenever central starts; the swap only compares digests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentStamp {
    organization: OrganizationId,
    digest: OrganizationStatementDigest,
}

impl CurrentStamp {
    #[must_use]
    pub fn of_stored(stored: &StoredOrganizationStamp) -> Self {
        Self {
            organization: stored.organization_id().clone(),
            digest: stored.digest(),
        }
    }
    #[must_use]
    pub fn of_verified(verified: &VerifiedFleetOrganizationStatement) -> Self {
        Self {
            organization: verified.organization_id().clone(),
            digest: verified.digest().clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrganizationStatementReplacement {
    Initialize,
    AlreadyProvisioned,
    Replace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OrganizationStatementReplacementRefusal {
    #[error("Fleet organization is immutable")]
    OrganizationMismatch,
    #[error("organization statement predecessor is stale or missing")]
    StaleOrganizationStatement,
}

pub fn decide_organization_statement_replacement(
    stored: Option<&CurrentStamp>,
    incoming: &VerifiedFleetOrganizationStatement,
) -> Result<OrganizationStatementReplacement, OrganizationStatementReplacementRefusal> {
    let Some(stored) = stored else {
        return match incoming.replaces() {
            None => Ok(OrganizationStatementReplacement::Initialize),
            Some(_) => Err(OrganizationStatementReplacementRefusal::StaleOrganizationStatement),
        };
    };
    if &stored.organization != incoming.organization_id() {
        return Err(OrganizationStatementReplacementRefusal::OrganizationMismatch);
    }
    // The byte-identical statement already stored is a replay of a committed
    // provisioning or renewal whose reply was lost: answer it, write nothing.
    if &stored.digest == incoming.digest() {
        return Ok(OrganizationStatementReplacement::AlreadyProvisioned);
    }
    match incoming.replaces() {
        None => Ok(OrganizationStatementReplacement::AlreadyProvisioned),
        Some(predecessor) if predecessor == &stored.digest => {
            Ok(OrganizationStatementReplacement::Replace)
        }
        Some(_) => Err(OrganizationStatementReplacementRefusal::StaleOrganizationStatement),
    }
}

#[cfg(test)]
#[path = "statement_replacement_test.rs"]
mod tests;
