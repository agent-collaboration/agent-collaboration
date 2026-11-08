//! The one-row organization stamp, read and written only inside a Fleet transaction.
use super::OrganizationStatementDigest;
use crate::{fleet_row_codec, generated_organization as queries};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_identity_protocol::OrganizationId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

/// The statement exactly as received, with the identity row it belongs to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredOrganizationStamp {
    organization: OrganizationId,
    statement: String,
}

impl StoredOrganizationStamp {
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

pub(crate) async fn read<S: TransactionScope>(scope: &mut S) -> Result<StoredStampRead, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let mut rows = queries::read_organization_identities(scope)
        .await
        .map_err(StoreError::backend)?;
    // The applying transaction keeps at most one row; a second is corruption,
    // never a choice between two organizations.
    if rows.len() > 1 {
        return Err(corrupt("id", "more than one organization identity row"));
    }
    let Some(row) = rows.pop() else {
        return Ok(StoredStampRead::Unprovisioned);
    };
    let id = text(row.id, row.id_class, "id")?;
    let organization = OrganizationId::try_from(id).map_err(|error| corrupt("id", error))?;
    let statement = text(
        row.organization_statement,
        row.organization_statement_class,
        "organization_statement",
    )?;
    if statement.is_empty() {
        return Err(corrupt("organization_statement", "statement is empty"));
    }
    Ok(StoredStampRead::Stamped(StoredOrganizationStamp {
        organization,
        statement,
    }))
}

pub(crate) async fn insert<S: TransactionScope>(
    scope: &mut S,
    organization: &OrganizationId,
    statement: &str,
    now_millis: i64,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
{
    queries::insert_organization_identity(
        scope,
        organization.as_str().into(),
        statement.into(),
        now_millis.to_string(),
        now_millis.to_string(),
    )
    .await
    .map_err(StoreError::backend)
}

/// Swap the statement only where the row still holds the expected predecessor.
pub(crate) async fn replace<S: TransactionScope>(
    scope: &mut S,
    stored: &StoredOrganizationStamp,
    statement: &str,
    now_millis: i64,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
{
    queries::replace_organization_statement(
        scope,
        statement.into(),
        now_millis.to_string(),
        stored.organization.as_str().into(),
        stored.statement.clone(),
    )
    .await
    .map_err(StoreError::backend)
}

/// Conformance only: replace whatever statement is stored, unverified.
#[cfg(feature = "conformance")]
pub(crate) async fn tamper<S: TransactionScope>(
    scope: &mut S,
    statement: &str,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let StoredStampRead::Stamped(stored) = read(scope).await? else {
        return Err(StoreError::backend("no stored statement to tamper with"));
    };
    replace(scope, &stored, statement, 0).await
}

fn text(value: Option<String>, class: Option<String>, field: &str) -> Result<String, StoreError> {
    if fleet_row_codec::required(class, field)? != "text" {
        return Err(corrupt(field, "column is not TEXT"));
    }
    fleet_row_codec::required(value, field)
}

fn corrupt(field: &str, message: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        field: format!("organization_identity.{field}"),
        message: message.to_string(),
    }
}
