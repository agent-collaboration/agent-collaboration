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
    // RED checkpoint only: tests must catch unconditional replacement.
    let _ = (stored, incoming);
    Ok(OrganizationStatementReplacement::Replace)
}

#[cfg(test)]
#[path = "statement_replacement_test.rs"]
mod tests;
