//! Organization agreement precedes the owning Fleet's existing directory read.
use super::VerifiedFleetOrganizationStatement;
use durable_steps::storage::TransactionScope;
use fleet_identity_protocol::OrganizationId;
use message_board::ProjectId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

/// The caller supplies a verified token organization and the addressed Fleet's
/// scope, and rechecks the stamp's time at its applying boundary.
pub async fn require_project_ownership<S: TransactionScope>(
    scope: &mut S,
    token_organization: &OrganizationId,
    stamp: &VerifiedFleetOrganizationStatement,
    project: &ProjectId,
) -> Result<(), ProjectOwnershipRefusal>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    if token_organization != stamp.organization_id() {
        return Err(ProjectOwnershipRefusal::OrganizationMismatch);
    }
    let present = crate::fleet_directory_store::read(scope, project.as_str())
        .await
        .map_err(|_| ProjectOwnershipRefusal::DirectoryUnavailable)?;
    match present {
        Some(_) => Ok(()),
        None => Err(ProjectOwnershipRefusal::ProjectUnavailable),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProjectOwnershipRefusal {
    #[error("token and Fleet organizations differ")]
    OrganizationMismatch,
    #[error("project is unavailable in this Fleet")]
    ProjectUnavailable,
    #[error("Fleet project directory is unavailable")]
    DirectoryUnavailable,
}

#[cfg(test)]
#[path = "project_ownership_test.rs"]
mod tests;
