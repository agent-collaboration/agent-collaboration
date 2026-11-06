//! Organization agreement precedes the owning Fleet's existing directory read.
use super::VerifiedFleetOrganizationStatement;
use durable_steps::storage::TransactionScope;
use fleet_identity_protocol::OrganizationId;
use message_board::ProjectId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

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
    // RED checkpoint: prove the tests catch ignoring ownership before completing the guard.
    let _ = (token_organization, stamp);
    let _present = crate::fleet_directory_store::read(scope, project.as_str())
        .await
        .map_err(|_| ProjectOwnershipRefusal::DirectoryUnavailable)?;
    Ok(())
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
