//! Pure replacement decisions take the statement read by the applying transaction.
use super::VerifiedFleetOrganizationStatement;

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
    stored: Option<&VerifiedFleetOrganizationStatement>,
    incoming: &VerifiedFleetOrganizationStatement,
) -> Result<OrganizationStatementReplacement, OrganizationStatementReplacementRefusal> {
    let Some(stored) = stored else {
        return match incoming.replaces() {
            None => Ok(OrganizationStatementReplacement::Initialize),
            Some(_) => Err(OrganizationStatementReplacementRefusal::StaleOrganizationStatement),
        };
    };
    if stored.organization_id() != incoming.organization_id() {
        return Err(OrganizationStatementReplacementRefusal::OrganizationMismatch);
    }
    match incoming.replaces() {
        None => Ok(OrganizationStatementReplacement::AlreadyProvisioned),
        Some(predecessor) if predecessor == stored.digest() => {
            Ok(OrganizationStatementReplacement::Replace)
        }
        Some(_) => Err(OrganizationStatementReplacementRefusal::StaleOrganizationStatement),
    }
}

#[cfg(test)]
#[path = "statement_replacement_test.rs"]
mod tests;
