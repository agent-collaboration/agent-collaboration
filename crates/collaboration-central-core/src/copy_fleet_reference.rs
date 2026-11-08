//! A Copy's central-private note of the organization whose Fleet it reports to.
//!
//! The note routes the Copy's own follow-up calls after a restart. It is not
//! replicated, and it grants nothing: project ownership is decided by the Fleet
//! directory before any project call is made. A note that names a different
//! organization than an arriving call still refuses that call, as a guard
//! against a routing bug reaching another organization's Copy.
use crate::generated_copy_fleet_reference as queries;
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_identity_protocol::OrganizationId;
use fleet_sync_protocol::DatabaseId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CopyFleetReference {
    Missing,
    Bound(OrganizationId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum CopyFleetReferenceRefusal {
    #[error("Copy is already bound to another Fleet")]
    OrganizationMismatch,
}

pub(crate) async fn resolve<S: TransactionScope>(
    scope: &mut S,
    project: DatabaseId,
) -> Result<CopyFleetReference, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    // A Copy whose schema has not been initialized yet has no note to read.
    if !queries::copy_fleet_reference_table_exists(scope)
        .await
        .map_err(StoreError::backend)?
    {
        return Ok(CopyFleetReference::Missing);
    }
    let Some(row) = queries::read_copy_fleet_reference(scope, project.to_string())
        .await
        .map_err(StoreError::backend)?
    else {
        return Ok(CopyFleetReference::Missing);
    };
    if row.project_id_class.as_deref() != Some("text")
        || row.project_id.as_deref() != Some(project.to_string().as_str())
        || row.fleet_address_key_class.as_deref() != Some("text")
    {
        return Err(corrupt("project_id", "note does not match its Copy"));
    }
    let organization = row
        .fleet_address_key
        .ok_or_else(|| corrupt("fleet_address_key", "missing"))?;
    OrganizationId::try_from(organization)
        .map(CopyFleetReference::Bound)
        .map_err(|error| corrupt("fleet_address_key", error))
}

/// Write the note once; the same organization repeating it changes nothing.
pub(crate) async fn bind<S: TransactionScope>(
    scope: &mut S,
    project: DatabaseId,
    organization: &OrganizationId,
    now_millis: i64,
) -> Result<Result<(), CopyFleetReferenceRefusal>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    match resolve(scope, project).await? {
        CopyFleetReference::Bound(bound) if &bound == organization => Ok(Ok(())),
        CopyFleetReference::Bound(_) => Ok(Err(CopyFleetReferenceRefusal::OrganizationMismatch)),
        CopyFleetReference::Missing => {
            queries::insert_copy_fleet_reference(
                scope,
                project.to_string(),
                organization.as_str().into(),
                now_millis.to_string(),
            )
            .await
            .map_err(StoreError::backend)?;
            Ok(Ok(()))
        }
    }
}

fn corrupt(field: &str, message: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        field: format!("_copy_fleet_reference.{field}"),
        message: message.to_string(),
    }
}

#[cfg(test)]
#[path = "copy_fleet_reference_test.rs"]
mod tests;

/// Read the note in its own Copy-store transaction.
pub(crate) struct ReadCopyFleetReference {
    pub project: DatabaseId,
}
/// Write the note in its own Copy-store transaction.
pub(crate) struct BindCopyFleetReference {
    pub project: DatabaseId,
    pub organization: OrganizationId,
    pub now_millis: i64,
}

impl<Store: durable_steps::AtomicStore, Driver: Database>
    crate::step_driver::StepCallback<Store, Driver> for ReadCopyFleetReference
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'v> String: Type<Driver> + Encode<'v, Driver>,
    for<'v> Option<String>: Type<Driver> + Decode<'v, Driver>,
    for<'r> &'r str: ColumnIndex<Driver::Row>,
{
    type Output = CopyFleetReference;
    type Effect = std::convert::Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> durable_steps::BoxFuture<'scope, crate::step_driver::StepResult<Self::Output, Self::Effect>>
    {
        Box::pin(async move { Ok((resolve(scope, self.project).await?, Vec::new())) })
    }
}

impl<Store: durable_steps::AtomicStore, Driver: Database>
    crate::step_driver::StepCallback<Store, Driver> for BindCopyFleetReference
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'v> String: Type<Driver> + Encode<'v, Driver>,
    for<'v> Option<String>: Type<Driver> + Decode<'v, Driver>,
    for<'r> &'r str: ColumnIndex<Driver::Row>,
{
    type Output = Result<(), CopyFleetReferenceRefusal>;
    type Effect = std::convert::Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> durable_steps::BoxFuture<'scope, crate::step_driver::StepResult<Self::Output, Self::Effect>>
    {
        Box::pin(async move {
            Ok((
                bind(scope, self.project, &self.organization, self.now_millis).await?,
                Vec::new(),
            ))
        })
    }
}
